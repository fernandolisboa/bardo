"""Download the pinned vision models and turn them into the ONNX files
Bardo ships (ADR-0024, issue #190).

    python crates/vision/tools/convert.py OUT_DIR

Every download is checked against its SHA-256 (crates/vision/models.toml).
MediaPipe's TFLite models are converted with tf2onnx and compared with the
TFLite interpreter on the same input; the depth checkpoints are loaded into
their upstream model code (cloned at a pinned commit) and exported with
torch.onnx, then compared with PyTorch. Each file's weights are then kept
as float16 and turned back into float32 when the model loads, which halves
the file and leaves the arithmetic in float32; the result is compared with
the float32 model again. Writes exactly the files the manifest's [[model]]
entries list, and SHA256SUMS.
"""

import argparse
import hashlib
import io
import pathlib
import shutil
import subprocess
import sys
import tomllib
import urllib.request
import zipfile

import numpy as np

HERE = pathlib.Path(__file__).resolve().parent
MANIFEST = tomllib.loads((HERE.parent / "models.toml").read_text())
SOURCES = MANIFEST["sources"]
DEPTH_SIZES = [(280, 504), (504, 280)]
FRAMES = 32


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def fetch(entry, cache):
    """The bytes of a manifest entry's download, from the cache when there."""
    url, want = entry["url"], entry["sha256"]
    path = cache / want
    if path.exists() and sha256(data := path.read_bytes()) == want:
        return data
    print(f"downloading {url}", flush=True)
    req = urllib.request.Request(url, headers={"User-Agent": "bardo-vision-models"})
    with urllib.request.urlopen(req) as r:
        data = r.read()
    got = sha256(data)
    if got != want:
        sys.exit(f"{url}: SHA-256 {got}, expected {want}")
    path.write_bytes(data)
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
    head = subprocess.run(["git", "-C", str(path), "rev-parse", "HEAD"], capture_output=True, text=True) if path.exists() else None
    if head is None or head.stdout.strip() != entry["commit"]:
        if path.exists():
            shutil.rmtree(path)
        print(f"cloning {entry['repo']} at {entry['commit']}", flush=True)
        subprocess.run(["git", "clone", "--quiet", "--filter=blob:none", "--no-checkout", entry["repo"], str(path)], check=True)
        subprocess.run(["git", "-C", str(path), "checkout", "--quiet", entry["commit"]], check=True)
    return path


def export_depth(work, out_dir, sizes, frames):
    import torch

    da2_code = checkout(SOURCES["depth_anything_v2_small"]["code"], work)
    vda_code = checkout(SOURCES["video_depth_anything_small"]["code"], work)
    da2_ckpt = work / "da2.pth"
    da2_ckpt.write_bytes(fetch(SOURCES["depth_anything_v2_small"], work))
    vda_ckpt = work / "vda.pth"
    vda_ckpt.write_bytes(fetch(SOURCES["video_depth_anything_small"], work))
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


def half_weights(path, inputs):
    """Keeps every float32 weight of the model at `path` as float16, with a
    Cast back to float32 in front of the graph: ONNX Runtime folds the
    casts when it loads the model, so it computes in float32 as before.
    Compares the result with the float32 model on `inputs` and rewrites the
    file."""
    import onnx
    import onnxruntime as ort
    from onnx import TensorProto, helper, numpy_helper

    sess = ort.InferenceSession(str(path), providers=["CPUExecutionProvider"])
    ref = sess.run(None, inputs)
    del sess
    model = onnx.load(str(path))
    graph = model.graph
    inputs_by_name = {i.name for i in graph.input}
    kept, casts, halved = [], [], 0
    for init in graph.initializer:
        if init.data_type != TensorProto.FLOAT or init.name in inputs_by_name:
            kept.append(init)
            continue
        values = numpy_helper.to_array(init)
        if values.size < 16:
            kept.append(init)
            continue
        name = init.name + "__float16"
        kept.append(numpy_helper.from_array(values.astype(np.float16), name))
        casts.append(helper.make_node("Cast", [name], [init.name], to=TensorProto.FLOAT, name=name + "_cast"))
        halved += values.size
    del graph.initializer[:]
    graph.initializer.extend(kept)
    nodes = list(graph.node)
    del graph.node[:]
    graph.node.extend(casts + nodes)
    onnx.checker.check_model(model, full_check=False)
    onnx.save(model, str(path))
    got = ort.InferenceSession(str(path), providers=["CPUExecutionProvider"]).run(None, inputs)
    worst = 0.0
    for r, g in zip(ref, got):
        span = float(np.abs(r).max()) or 1.0
        worst = max(worst, float(np.abs(g - r).max()) / span)
    print(f"{path.name}: {halved} weights as float16, max difference from float32 {worst:.2e} of the range")
    if worst > 5e-3:
        sys.exit(f"{path.name}: float16 weights change the output too much")


def random_inputs(path):
    import onnxruntime as ort

    sess = ort.InferenceSession(str(path), providers=["CPUExecutionProvider"])
    rng = np.random.default_rng(1)
    out = {}
    for i in sess.get_inputs():
        shape = [d if isinstance(d, int) else 1 for d in i.shape]
        x = rng.random(shape, dtype=np.float32)
        if "blendshapes" in path.name:
            # Landmark positions in pixels of the 256 crop.
            x = x * 256.0
        out[i.name] = x
    return out


def main():
    p = argparse.ArgumentParser()
    p.add_argument("out_dir", type=pathlib.Path)
    a = p.parse_args()
    out = a.out_dir
    work = out / ".work"
    work.mkdir(parents=True, exist_ok=True)

    task = fetch(SOURCES["face_landmarker"], work)
    with zipfile.ZipFile(io.BytesIO(task)) as z:
        for name in ["face_detector", "face_landmarks_detector", "face_blendshapes"]:
            tfl = work / f"{name}.tflite"
            tfl.write_bytes(z.read(f"{name}.tflite"))
            convert_tflite(tfl, out / f"{name}.onnx")
    seg = work / "selfie_multiclass_256x256.tflite"
    seg.write_bytes(fetch(SOURCES["selfie_multiclass"], work))
    convert_tflite(seg, out / "selfie_multiclass_256x256.onnx")
    export_depth(work, out, DEPTH_SIZES, FRAMES)

    wanted = [m["file"] for m in MANIFEST["model"]]
    written = sorted(f.name for f in out.glob("*.onnx"))
    if sorted(wanted) != written:
        sys.exit(f"wrote {written}, the manifest lists {sorted(wanted)}")
    for name in wanted:
        path = out / name
        half_weights(path, random_inputs(path))

    sums = []
    print("\nSHA-256 of the files written:")
    for name in wanted:
        f = out / name
        digest = sha256(f.read_bytes())
        sums.append(f"{digest}  {name}\n")
        print(f"{digest}  {name}  {f.stat().st_size} bytes")
    (out / "SHA256SUMS").write_text("".join(sums))


if __name__ == "__main__":
    main()
