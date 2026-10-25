"""Run upstream LivePortrait (PyTorch, CPU, float32, its InsightFace crop)
on a look and a take, for comparison with liveportrait-probe.

    python tools/reference.py LIVEPORTRAIT_CODE WEIGHTS TAKE OUT_DIR
        (--look IMAGE | --self) [--frames N]

LIVEPORTRAIT_CODE and WEIGHTS are what convert.py fetches into
OUT/.work/LivePortrait and OUT/.work/weights (with the [reference.files]
of models.toml: InsightFace's models, for non-commercial research only,
run here and never shipped). Writes OUT_DIR/reference.mp4 and
OUT_DIR/reference.json: the crops upstream chose and, with --self (the
take's first frame animated by the take), how close each frame comes to the
take's own frame, measured as liveportrait-probe measures it.
"""

import argparse
import json
import pathlib
import shutil
import subprocess
import sys
import time

import numpy as np


def psnr(sum_sq, n):
    return float(10 * np.log10(255.0**2 / max(sum_sq / max(n, 1.0), 1e-12)))


def main():
    p = argparse.ArgumentParser()
    p.add_argument("code", type=pathlib.Path)
    p.add_argument("weights", type=pathlib.Path)
    p.add_argument("take", type=pathlib.Path)
    p.add_argument("out", type=pathlib.Path)
    p.add_argument("--look", type=pathlib.Path)
    p.add_argument("--self", dest="self_", action="store_true")
    p.add_argument("--frames", type=int)
    p.add_argument("--ffmpeg", default="ffmpeg")
    a = p.parse_args()
    if bool(a.look) == a.self_:
        sys.exit("give --look IMAGE or --self")
    a.out.mkdir(parents=True, exist_ok=True)
    work = a.out / "reference-work"
    shutil.rmtree(work, ignore_errors=True)
    work.mkdir()

    # Upstream reads its weights from pretrained_weights/ in its own tree.
    link = a.code / "pretrained_weights"
    if link.is_symlink() or not link.exists():
        link.unlink(missing_ok=True)
        link.symlink_to(a.weights.resolve())
    else:
        shutil.rmtree(link)
        link.symlink_to(a.weights.resolve())

    run = lambda *args: subprocess.run([a.ffmpeg, "-v", "error", "-y", *args], check=True)
    take = work / "take.mp4"
    if a.frames:
        run("-i", str(a.take), "-frames:v", str(a.frames), "-c:v", "libx264", "-crf", "10", "-c:a", "aac", "-shortest", str(take))
    else:
        shutil.copy(a.take, take)
    if a.self_:
        look = work / "look.png"
        run("-i", str(a.take), "-frames:v", "1", str(look))
    else:
        look = work / ("look" + a.look.suffix)
        shutil.copy(a.look, look)

    sys.path.insert(0, str(a.code))
    import cv2
    import src.live_portrait_pipeline as lpp
    import src.utils.cropper as cropper
    from src.config.argument_config import ArgumentConfig
    from src.config.crop_config import CropConfig
    from src.config.inference_config import InferenceConfig

    captured = {"masks": [], "pasted": [], "crops": []}
    prepare, paste, average = lpp.prepare_paste_back, lpp.paste_back, cropper.average_bbox_lst

    def prepare_paste_back(mask_crop, crop_m_c2o, dsize):
        captured["crops"].append(np.asarray(crop_m_c2o, dtype=np.float64))
        mask = prepare(mask_crop, crop_m_c2o, dsize)
        captured["masks"].append(mask)
        return mask

    def paste_back(*args):
        out = paste(*args)
        captured["pasted"].append(out)
        return out

    def average_bbox_lst(boxes):
        box = average(boxes)
        captured["driving_box"] = [float(v) for v in box]
        return box

    lpp.prepare_paste_back, lpp.paste_back = prepare_paste_back, paste_back
    cropper.average_bbox_lst = average_bbox_lst

    inference = InferenceConfig(flag_force_cpu=True, flag_use_half_precision=False)
    crop = CropConfig(flag_force_cpu=True)
    pipeline = lpp.LivePortraitPipeline(inference_cfg=inference, crop_cfg=crop)
    started = time.time()
    pipeline.execute(ArgumentConfig(source=str(look), driving=str(take), output_dir=str(work), flag_force_cpu=True, flag_use_half_precision=False))
    wall = time.time() - started

    frames = captured["pasted"]
    h, w = frames[0].shape[:2]
    report = {
        "frames": len(frames),
        "look": [w, h],
        "wall_s": wall,
        "s_per_frame": wall / len(frames),
        "source_crop": np.linalg.inv(captured["crops"][0])[:2].tolist(),
        "driving_box": captured.get("driving_box"),
    }
    if a.self_:
        cap = cv2.VideoCapture(str(take))
        face, whole = [0.0, 0.0], [0.0, 0.0]
        mask = captured["masks"][0]
        for out in frames:
            ok, bgr = cap.read()
            if not ok:
                break
            truth = cv2.resize(cv2.cvtColor(bgr, cv2.COLOR_BGR2RGB), (w, h), interpolation=cv2.INTER_LINEAR)
            e = (out.astype(np.float64) - truth.astype(np.float64)) ** 2
            face[0] += float((mask * e).sum())
            face[1] += float(mask.sum())
            whole[0] += float(e.sum())
            whole[1] += e.size
        report["self_psnr"] = {"face_db": psnr(*face), "whole_frame_db": psnr(*whole)}

    writer = subprocess.Popen(
        [a.ffmpeg, "-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", f"{w}x{h}", "-r", "30", "-i", "-",
         "-c:v", "libx264", "-crf", "16", "-pix_fmt", "yuv420p", str(a.out / "reference.mp4")],
        stdin=subprocess.PIPE,
    )
    for f in frames:
        writer.stdin.write(np.ascontiguousarray(f).tobytes())
    writer.stdin.close()
    writer.wait()
    (a.out / "reference.json").write_text(json.dumps(report, indent=2))
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
