"""Download the pinned vision models and turn them into the ONNX files
vision-probe runs (issue #187).

    python tools/convert.py OUT_DIR [--depth-size 280x504 ...]

Every download is checked against its SHA-256 (models.toml). MediaPipe's
TFLite models are converted with tf2onnx and compared with the TFLite
interpreter on the same input; the depth checkpoints are loaded into their
upstream model code (cloned at a pinned commit) and exported with
torch.onnx, then compared with PyTorch. The SHA-256 of every file written
is printed at the end.
"""

import argparse
import hashlib
import io
import pathlib
import subprocess
import sys
import tomllib
import urllib.request
import zipfile

import numpy as np

HERE = pathlib.Path(__file__).resolve().parent
MANIFEST = tomllib.loads((HERE.parent / "models.toml").read_text())


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def fetch(entry, cache):
    """The bytes of a manifest entry's download, from the cache when there."""
    url, want = entry["url"], entry["sha256"]
    path = cache / want
    if path.exists():
        data = path.read_bytes()
    else:
        print(f"downloading {url}", flush=True)
        req = urllib.request.Request(url, headers={"User-Agent": "bardo-vision-probe"})
        with urllib.request.urlopen(req) as r:
            data = r.read()
        path.write_bytes(data)
    got = sha256(data)
    if got != want:
        sys.exit(f"{url}: SHA-256 {got}, expected {want}")
    return data


def tflite_reference(path, x):
    import tensorflow as tf

    it = tf.lite.Interpreter(model_path=str(path))
    it.allocate_tensors()
    it.set_tensor(it.get_input_details()[0]["index"], x)
    it.invoke()
    return [it.get_tensor(d["index"]) for d in it.get_output_details()]


def fix_nearest_resize(model):
    """tf2onnx writes TensorFlow's nearest-neighbour upsampling as Resize
    with `tf_half_pixel_for_nn`, deprecated since opset 13. For whole-number
    upscales it picks the same pixels as `asymmetric` with `floor`."""
    n = 0
    for node in model.graph.node:
        if node.op_type != "Resize":
            continue
        for a in node.attribute:
            if a.name == "coordinate_transformation_mode" and a.s == b"tf_half_pixel_for_nn":
                a.s = b"asymmetric"
                n += 1
            if a.name == "nearest_mode":
                a.s = b"floor"
    return n


def convert_tflite(tflite, out):
    import onnx
    import onnxruntime as ort
    import tf2onnx

    model, _ = tf2onnx.convert.from_tflite(str(tflite), opset=17)
    fixed = fix_nearest_resize(model)
    onnx.save(model, str(out))
    sess = ort.InferenceSession(str(out), providers=["CPUExecutionProvider"])
    inp = sess.get_inputs()[0]
    shape = [d if isinstance(d, int) else 1 for d in inp.shape]
    x = np.random.default_rng(0).random(shape, dtype=np.float32)
    ref = tflite_reference(tflite, x)
    got = sess.run(None, {inp.name: x})
    worst = 0.0
    for r in ref:
        match = [g for g in got if g.size == r.size]
        worst = max(worst, min(float(np.abs(g.reshape(-1) - r.reshape(-1)).max()) for g in match))
    print(f"{out.name}: max difference from TFLite {worst:.2e}, Resize nodes fixed {fixed}")
    if worst > 1e-3:
        sys.exit(f"{out.name} does not match TFLite")


def checkout(entry, dest):
    """The upstream model code at a pinned commit; the commit id pins the
    content, as a SHA-256 does for a download."""
    path = dest / entry["commit"]
    if not path.exists():
        print(f"cloning {entry['repo']} at {entry['commit']}", flush=True)
        subprocess.run(["git", "clone", "--quiet", "--filter=blob:none", "--no-checkout", entry["repo"], str(path)], check=True)
        subprocess.run(["git", "-C", str(path), "checkout", "--quiet", entry["commit"]], check=True)
    return path


def export_depth(work, out_dir, sizes, frames):
    import torch

    da2_code = checkout(MANIFEST["depth_anything_v2_small"]["code"], work)
    vda_code = checkout(MANIFEST["video_depth_anything_small"]["code"], work)
    da2_ckpt = work / "da2.pth"
    da2_ckpt.write_bytes(fetch(MANIFEST["depth_anything_v2_small"], work))
    vda_ckpt = work / "vda.pth"
    vda_ckpt.write_bytes(fetch(MANIFEST["video_depth_anything_small"], work))
    cfg = dict(features=64, out_channels=[48, 96, 192, 384])

    sys.path.insert(0, str(da2_code))
    from depth_anything_v2.dpt import DepthAnythingV2

    da2 = DepthAnythingV2(encoder="vits", **cfg)
    da2.load_state_dict(torch.load(da2_ckpt, map_location="cpu"))
    da2.eval()
    sys.path.pop(0)

    sys.path.insert(0, str(vda_code))
    from video_depth_anything.motion_module.attention import CrossAttention
    from video_depth_anything.video_depth import VideoDepthAnything

    # Upstream attention starts from torch.baddbmm over an uninitialised
    # tensor with beta=0. Traced, that tensor becomes a constant of
    # N x 32 x 32 values per attention block (about 300 MB at 280x504).
    # A plain matmul is the same sum.
    def _attention(self, query, key, value, attention_mask=None):
        scores = torch.matmul(query, key.transpose(-1, -2)) * self.scale
        if attention_mask is not None:
            scores = scores + attention_mask
        return self.reshape_batch_dim_to_heads(torch.bmm(scores.softmax(dim=-1), value))

    CrossAttention._attention = _attention
    vda = VideoDepthAnything(encoder="vits", **cfg)
    vda.load_state_dict(torch.load(vda_ckpt, map_location="cpu"), strict=True)
    vda.eval()

    import onnxruntime as ort

    for h, w in sizes:
        for name, model, shape, inp in [
            (f"depth_anything_v2_small_{h}x{w}.onnx", da2, (1, 3, h, w), "image"),
            (f"video_depth_anything_small_{frames}x{h}x{w}.onnx", vda, (1, frames, 3, h, w), "frames"),
        ]:
            x = torch.randn(*shape, generator=torch.Generator().manual_seed(0))
            out = out_dir / name
            with torch.no_grad():
                torch.onnx.export(model, (x,), str(out), input_names=[inp], output_names=["depth"], opset_version=17, dynamo=False)
                ref = model(x).numpy()
            got = ort.InferenceSession(str(out), providers=["CPUExecutionProvider"]).run(None, {inp: x.numpy()})[0]
            rel = float(np.abs(got - ref).max() / np.abs(ref).max())
            print(f"{name}: max difference from PyTorch {rel:.2e} of the range")
            if rel > 1e-3:
                sys.exit(f"{name} does not match PyTorch")


def main():
    p = argparse.ArgumentParser()
    p.add_argument("out_dir", type=pathlib.Path)
    p.add_argument("--depth-size", action="append", default=None, help="HEIGHTxWIDTH, multiples of 14")
    p.add_argument("--frames", type=int, default=32)
    p.add_argument("--skip-depth", action="store_true")
    a = p.parse_args()
    out = a.out_dir
    work = out / ".work"
    work.mkdir(parents=True, exist_ok=True)

    task = fetch(MANIFEST["face_landmarker"], work)
    with zipfile.ZipFile(io.BytesIO(task)) as z:
        for name in ["face_detector", "face_landmarks_detector", "face_blendshapes"]:
            tfl = work / f"{name}.tflite"
            tfl.write_bytes(z.read(f"{name}.tflite"))
            convert_tflite(tfl, out / f"{name}.onnx")
    seg = work / "selfie_multiclass_256x256.tflite"
    seg.write_bytes(fetch(MANIFEST["selfie_multiclass"], work))
    convert_tflite(seg, out / "selfie_multiclass_256x256.onnx")

    if not a.skip_depth:
        sizes = [tuple(int(v) for v in s.split("x")) for s in (a.depth_size or ["280x504"])]
        export_depth(work, out, sizes, a.frames)

    print("\nSHA-256 of the files written:")
    for f in sorted(out.glob("*.onnx")):
        print(f"{sha256(f.read_bytes())}  {f.name}  {f.stat().st_size} bytes")


if __name__ == "__main__":
    main()
