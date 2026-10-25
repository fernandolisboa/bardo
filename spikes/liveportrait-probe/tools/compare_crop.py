"""Compare the crops liveportrait-probe takes from MediaPipe's mesh with
upstream's, which come from InsightFace and LivePortrait's 203-point
landmark model.

    python tools/compare_crop.py LIVEPORTRAIT_CODE WEIGHTS FACE_LANDMARKER_TASK OUT.json VIDEO_OR_IMAGE...

LIVEPORTRAIT_CODE and WEIGHTS as for reference.py (InsightFace's models are
for non-commercial research: run here, never shipped). The MediaPipe points
and the crop arithmetic are read from src/crop.rs, so this measures what
the probe does. For each frame: the crop's center offset and size as a
share of upstream's crop size, and the tilt difference in degrees, for the
source crop (turned with the face) and the driving box (upright); and the
lip-open ratio from both sets of points.
"""

import json
import pathlib
import re
import sys

import numpy as np

HERE = pathlib.Path(__file__).resolve().parent


def mediapipe_indices():
    src = (HERE.parent / "src" / "crop.rs").read_text()

    def const(name):
        body = re.search(rf"const {name}: \[usize; \d+\] = \[(.*?)\];", src, re.S).group(1)
        body = re.sub(r"//[^\n]*", "", body)
        return [int(v) for v in re.findall(r"\d+", body)]

    return {k: const(k) for k in ["MP_LEFT_EYE", "MP_RIGHT_EYE", "MP_MOUTH_CORNERS", "MP_INNER_LIPS", "MP_EXTENT"]}


MP = mediapipe_indices()


def mp_face(mesh):
    """FacePoints::from_mediapipe."""
    eyes = (mesh[MP["MP_LEFT_EYE"]].mean(0) + mesh[MP["MP_RIGHT_EYE"]].mean(0)) / 2
    mouth = mesh[MP["MP_MOUTH_CORNERS"]].mean(0)
    return mesh[MP["MP_EXTENT"]], eyes, mouth


def mp_lip_ratio(mesh):
    a, b = MP["MP_INNER_LIPS"]
    c, d = MP["MP_MOUTH_CORNERS"]
    return float(np.linalg.norm(mesh[a] - mesh[b]) / (np.linalg.norm(mesh[c] - mesh[d]) + 1e-6))


def rect(points, eyes, mouth, scale, vy):
    """crop::rect, as upstream's parse_rect_from_landmark."""
    uy = mouth - eyes
    n = np.linalg.norm(uy)
    uy = np.array([0.0, 1.0]) if n <= 1e-3 else uy / n
    ux = np.array([uy[1], -uy[0]])
    angle = np.arccos(np.clip(ux[0], -1, 1)) * (1 if ux[1] >= 0 else -1)
    c0 = points.mean(0)
    r = (points - c0) @ np.stack([ux, uy]).T
    lo, hi = r.min(0), r.max(0)
    c1 = (lo + hi) / 2
    size = (hi - lo).max() * scale
    center = c0 + ux * c1[0] + uy * c1[1] + uy * vy * size
    return center, size, angle


def frames(path, cv2):
    if path.suffix.lower() in (".jpg", ".jpeg", ".png"):
        yield cv2.cvtColor(cv2.imread(str(path)), cv2.COLOR_BGR2RGB)
        return
    cap = cv2.VideoCapture(str(path))
    while True:
        ok, bgr = cap.read()
        if not ok:
            return
        yield cv2.cvtColor(bgr, cv2.COLOR_BGR2RGB)


def summary(values):
    v = np.abs(np.asarray(values, dtype=np.float64))
    return {"mean": float(v.mean()), "p95": float(np.percentile(v, 95)), "max": float(v.max())}


def main(code, weights, task, out, *inputs):
    code, weights = pathlib.Path(code), pathlib.Path(weights)
    link = code / "pretrained_weights"
    if not link.exists():
        link.symlink_to(weights.resolve())
    sys.path.insert(0, str(code))
    import cv2
    import mediapipe as mp
    from src.config.crop_config import CropConfig
    from src.utils.cropper import Cropper
    from src.utils.crop import parse_pt2_from_pt203
    from src.utils.retargeting_utils import calc_lip_close_ratio

    cfg = CropConfig(flag_force_cpu=True)
    cropper = Cropper(crop_cfg=cfg, flag_force_cpu=True)
    report = {}
    for name in inputs:
        path = pathlib.Path(name)
        video = path.suffix.lower() not in (".jpg", ".jpeg", ".png")
        options = mp.tasks.vision.FaceLandmarkerOptions(
            base_options=mp.tasks.BaseOptions(model_asset_path=task),
            running_mode=mp.tasks.vision.RunningMode.VIDEO if video else mp.tasks.vision.RunningMode.IMAGE,
        )
        landmarker = mp.tasks.vision.FaceLandmarker.create_from_options(options)
        rows = {k: [] for k in ["source_center", "source_size", "source_angle", "driving_center", "driving_size", "lip_203", "lip_mp"]}
        last = None
        boxes = {"203": [], "mp": []}
        for i, rgb in enumerate(frames(path, cv2)):
            h, w = rgb.shape[:2]
            image = mp.Image(image_format=mp.ImageFormat.SRGB, data=np.ascontiguousarray(rgb))
            found = landmarker.detect_for_video(image, int(i * 1000 / 30)) if video else landmarker.detect(image)
            if not found.face_landmarks:
                continue
            mesh = np.array([[p.x * w, p.y * h] for p in found.face_landmarks[0]])
            # Upstream: InsightFace finds the face once, then its landmark
            # model tracks the 203 points from the last frame's.
            if last is None:
                faces = cropper.face_analysis_wrapper.get(cv2.cvtColor(rgb, cv2.COLOR_RGB2BGR), flag_do_landmark_2d_106=True, direction=cfg.direction)
                if not faces:
                    continue
                last = cropper.human_landmark_runner.run(rgb, faces[0].landmark_2d_106)
            else:
                last = cropper.human_landmark_runner.run(rgb, last)
            pts = np.asarray(last, dtype=np.float64)
            pt2 = parse_pt2_from_pt203(pts)
            ref = {
                "source": rect(pts, pt2[0], pt2[1], cfg.scale, cfg.vy_ratio),
                "driving": rect(pts, pt2[0], pt2[1], cfg.scale_crop_driving_video, cfg.vy_ratio_crop_driving_video),
            }
            points, eyes, mouth = mp_face(mesh)
            got = {
                "source": rect(points, eyes, mouth, cfg.scale, cfg.vy_ratio),
                "driving": rect(points, eyes, mouth, cfg.scale_crop_driving_video, cfg.vy_ratio_crop_driving_video),
            }
            for kind in ["source", "driving"]:
                (c_ref, s_ref, a_ref), (c_got, s_got, a_got) = ref[kind], got[kind]
                rows[f"{kind}_center"].append(float(np.linalg.norm(c_got - c_ref) / s_ref))
                rows[f"{kind}_size"].append(float(s_got / s_ref - 1))
                if kind == "source":
                    rows["source_angle"].append(float(np.degrees(a_got - a_ref)))
            for key, (c, s, _) in [("203", ref["driving"]), ("mp", got["driving"])]:
                boxes[key].append([c[0] - s / 2, c[1] - s / 2, c[0] + s / 2, c[1] + s / 2])
            rows["lip_203"].append(float(calc_lip_close_ratio(pts[None])[0, 0]))
            rows["lip_mp"].append(mp_lip_ratio(mesh))
        landmarker.close()
        entry = {"frames": len(rows["lip_mp"])}
        for k in ["source_center", "source_size", "source_angle", "driving_center", "driving_size"]:
            entry[k] = summary(rows[k])
        entry["size_bias"] = {k: float(np.mean(rows[f"{k}_size"])) for k in ["source", "driving"]}
        lip203, lipmp = np.array(rows["lip_203"]), np.array(rows["lip_mp"])
        entry["lip_ratio"] = {
            "mean_203": float(lip203.mean()), "mean_mediapipe": float(lipmp.mean()),
            "correlation": float(np.corrcoef(lip203, lipmp)[0, 1]) if len(lip203) > 2 else None,
            "closed_agree": float(np.mean((lip203 >= 0.03) == (lipmp >= 0.03))),
        }
        if video:
            b203, bmp = np.mean(boxes["203"], 0), np.mean(boxes["mp"], 0)
            size = b203[2] - b203[0]
            entry["driving_mean_box"] = {
                "center_offset": float(np.linalg.norm((bmp[:2] + bmp[2:]) / 2 - (b203[:2] + b203[2:]) / 2) / size),
                "size": float((bmp[2] - bmp[0]) / size - 1),
            }
        report[path.name] = entry
        print(path.name, json.dumps(entry, indent=1), flush=True)
    pathlib.Path(out).write_text(json.dumps(report, indent=2))


if __name__ == "__main__":
    main(*sys.argv[1:])
