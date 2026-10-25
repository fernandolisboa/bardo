"""Download LivePortrait's pinned code and weights and export its networks
to ONNX (issue #242).

    python tools/convert.py OUT_DIR

Every download is checked against its SHA-256 (models.toml); the code is
cloned at its pinned commit. Each network is loaded into the upstream
model code, exported with torch.onnx and compared with PyTorch on the same
input. Writes, in OUT_DIR:

- appearance_feature_extractor.onnx, motion_extractor.onnx,
  spade_generator.onnx, stitching.onnx, retarget_lip.onnx,
  retarget_eye.onnx;
- warping_module.onnx, with the two 5-D `grid_sample` calls rewritten into
  Gather and arithmetic (opset 17: every execution provider has those);
- warping_module_gridsample.onnx, the same with ONNX's own GridSample
  (opset 20, the first that takes 5-D input);
- mask_template.png, upstream's paste-back mask, and lip_array.json, the
  closed-lip expression upstream subtracts for a still driving image;
- SHA256SUMS.

Then every file's weights are kept as float16 and turned back into float32
when the model loads (as crates/vision does), and the result is compared
with the float32 file again. `--fp16` also writes `*_fp16.onnx` copies of
the warping module and the generator that compute in float16 (inputs and
outputs stay float32), for DirectML; ONNX Runtime's CPU has no float16
kernels for most of their operators.
"""

import argparse
import hashlib
import json
import pathlib
import pickle
import shutil
import subprocess
import sys
import tomllib
import urllib.request

import numpy as np

HERE = pathlib.Path(__file__).resolve().parent
MANIFEST = tomllib.loads((HERE.parent / "models.toml").read_text())
TOLERANCE = 1e-3


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        while chunk := f.read(1 << 20):
            h.update(chunk)
    return h.hexdigest()


def fetch(base, name, want, cache):
    """The path of a pinned download, from the cache when it is there."""
    path = cache / name
    if path.exists() and sha256(path) == want:
        return path
    path.parent.mkdir(parents=True, exist_ok=True)
    part = path.with_name(path.name + ".part")
    url = f"{base}/{name}"
    print(f"downloading {url}", flush=True)
    req = urllib.request.Request(url, headers={"User-Agent": "bardo-liveportrait-probe"})
    with urllib.request.urlopen(req) as r, open(part, "wb") as f:
        while chunk := r.read(1 << 20):
            f.write(chunk)
    got = sha256(part)
    if got != want:
        part.unlink()
        sys.exit(f"{url}: SHA-256 {got}, expected {want}")
    part.replace(path)
    return path


def clone(work):
    code = MANIFEST["code"]
    dest = work / "LivePortrait"
    if not dest.exists():
        subprocess.run(["git", "clone", "-q", code["repo"], str(dest)], check=True)
    subprocess.run(["git", "-C", str(dest), "checkout", "-q", code["commit"]], check=True)
    return dest


def fetch_weights(work, section):
    base = MANIFEST["weights"]["base"]
    return {name: fetch(base, name, want, work / "weights") for name, want in MANIFEST[section]["files"].items()}


def grid_sample_3d(inp, grid):
    """`F.grid_sample` on 5-D input (trilinear, zero padding,
    align_corners=False), made of Gather and arithmetic only. The input is
    laid out a voxel per row, so a single Gather reads the eight corners of
    every sample with all their channels."""
    import torch

    n, c, d, h, w = inp.shape
    _, do, ho, wo, _ = grid.shape
    p = do * ho * wo
    g = grid.reshape(n, p, 3)
    x = ((g[..., 0] + 1) * w - 1) / 2
    y = ((g[..., 1] + 1) * h - 1) / 2
    z = ((g[..., 2] + 1) * d - 1) / 2
    x0, y0, z0 = torch.floor(x), torch.floor(y), torch.floor(z)
    fx, fy, fz = x - x0, y - y0, z - z0
    table = inp.permute(0, 2, 3, 4, 1).reshape(n * d * h * w, c)
    base = (torch.arange(n, dtype=inp.dtype) * (d * h * w)).reshape(n, 1)
    indices, weights = [], []
    for dz, wz in ((0, 1 - fz), (1, fz)):
        zi = z0 + dz
        inside_z = (zi >= 0) & (zi <= d - 1)
        for dy, wy in ((0, 1 - fy), (1, fy)):
            yi = y0 + dy
            inside_y = inside_z & (yi >= 0) & (yi <= h - 1)
            for dx, wx in ((0, 1 - fx), (1, fx)):
                xi = x0 + dx
                inside = inside_y & (xi >= 0) & (xi <= w - 1)
                index = (zi.clamp(0, d - 1) * h + yi.clamp(0, h - 1)) * w + xi.clamp(0, w - 1) + base
                indices.append(index)
                weights.append(wz * wy * wx * inside.to(inp.dtype))
    index = torch.stack(indices).reshape(-1).long()
    weight = torch.stack(weights).reshape(8, n * p, 1)
    corners = table[index].reshape(8, n * p, c)
    out = (corners * weight).sum(0)
    return out.reshape(n, do, ho, wo, c).permute(0, 4, 1, 2, 3).contiguous()


def gather_grid_sample(original):
    def patched(inp, grid, *args, **kwargs):
        if inp.dim() != 5:
            return original(inp, grid, *args, **kwargs)
        mode = kwargs.get("mode", args[0] if args else "bilinear")
        padding = kwargs.get("padding_mode", args[1] if len(args) > 1 else "zeros")
        if mode != "bilinear" or padding != "zeros" or kwargs.get("align_corners") is not False:
            raise ValueError("only trilinear, zero padding, align_corners=False is rewritten")
        return grid_sample_3d(inp, grid)

    return patched


def load_networks(code, weights):
    import torch
    import yaml

    sys.path.insert(0, str(code))
    from src.utils.helper import load_model

    config = yaml.safe_load((code / "src/config/models.yaml").read_text())
    base = "liveportrait/base_models/"
    nets = {
        kind: load_model(str(weights[f"{base}{kind}.pth"]), config, "cpu", kind)
        for kind in ["appearance_feature_extractor", "motion_extractor", "warping_module", "spade_generator"]
    }
    nets.update(load_model(str(weights["liveportrait/retargeting_models/stitching_retargeting_module.pth"]), config, "cpu", "stitching_retargeting_module"))
    torch.set_grad_enabled(False)
    # The generator's SPADE blocks keep their convolutions under spectral
    # norm: traced as is, each weight is divided by its norm at run time and
    # the file carries the weight twice. Folding the norm in once gives the
    # same weights (eval mode does no power iteration) in half the size.
    for m in nets["spade_generator"].modules():
        if hasattr(m, "weight_orig"):
            torch.nn.utils.remove_spectral_norm(m)
    return nets


def check(path, feeds, reference):
    import onnxruntime as ort

    sess = ort.InferenceSession(str(path), providers=["CPUExecutionProvider"])
    got = sess.run(None, {k: v.numpy() for k, v in feeds.items()})
    worst = 0.0
    for g, r in zip(got, reference):
        r = r.numpy()
        span = float(np.abs(r).max()) or 1.0
        worst = max(worst, float(np.abs(g - r).max()) / span)
    print(f"{path.name}: max difference from PyTorch {worst:.2e} of the range", flush=True)
    if worst > TOLERANCE:
        sys.exit(f"{path.name} does not match PyTorch")


def export(module, feeds, outputs, path, opset):
    import torch

    # torch.onnx.export puts the module back in the mode it found it in, and
    # a wrapper made in training mode would leave the BatchNorm layers inside
    # it computing batch statistics from then on.
    module.eval()
    reference = module(*feeds.values())
    if not isinstance(reference, tuple):
        reference = (reference,)
    torch.onnx.export(
        module,
        tuple(feeds.values()),
        str(path),
        input_names=list(feeds),
        output_names=outputs,
        opset_version=opset,
        dynamo=False,
    )
    check(path, feeds, reference)


def face_image():
    """A face to feed the networks, framed about as LivePortrait frames
    one: a fixed square of the Mona Lisa look, 256 × 256, RGB from 0 to 1.
    Random noise is no face, and the motion extractor's float16 error on
    noise says little about its error on a face."""
    import cv2
    import torch

    bgr = cv2.imread(str(HERE.parent / "looks" / "mona-lisa.jpg"))
    rgb = cv2.cvtColor(cv2.resize(bgr[30:350, 150:470], (256, 256), interpolation=cv2.INTER_AREA), cv2.COLOR_BGR2RGB)
    return torch.from_numpy(rgb.astype(np.float32) / 255).permute(2, 0, 1)[None].contiguous()


def degrees(logits):
    """Upstream's head angle from the motion extractor's 66 bins."""
    e = np.exp(logits - logits.max(axis=1, keepdims=True))
    p = e / e.sum(axis=1, keepdims=True)
    return (p * np.arange(66)).sum(axis=1) * 3 - 97.5


def export_all(nets, out):
    import torch
    import torch.nn.functional as F

    class Motion(torch.nn.Module):
        """The motion extractor's dict as a tuple, in a fixed order."""

        KEYS = ["pitch", "yaw", "roll", "t", "exp", "scale", "kp"]

        def __init__(self, net):
            super().__init__()
            self.net = net

        def forward(self, image):
            out = self.net(image)
            return tuple(out[k] for k in self.KEYS)

    class Warp(torch.nn.Module):
        def __init__(self, net):
            super().__init__()
            self.net = net

        def forward(self, feature_3d, kp_driving, kp_source):
            return self.net(feature_3d, kp_driving=kp_driving, kp_source=kp_source)["out"]

    gen = torch.Generator().manual_seed(1)
    image = face_image()
    export(nets["appearance_feature_extractor"], {"image": image}, ["feature_3d"], out / "appearance_feature_extractor.onnx", 17)
    export(Motion(nets["motion_extractor"]), {"image": image}, Motion.KEYS, out / "motion_extractor.onnx", 17)

    # Keypoints as LivePortrait makes them: within about ±0.5, the driving
    # ones near the source's.
    feature = nets["appearance_feature_extractor"](image)
    kp_source = torch.rand(1, 21, 3, generator=gen) - 0.5
    kp_driving = kp_source + 0.05 * torch.randn(1, 21, 3, generator=gen)
    warp_feeds = {"feature_3d": feature, "kp_driving": kp_driving, "kp_source": kp_source}
    export(Warp(nets["warping_module"]), warp_feeds, ["out"], out / "warping_module_gridsample.onnx", 20)
    original = F.grid_sample
    F.grid_sample = gather_grid_sample(original)
    try:
        export(Warp(nets["warping_module"]), warp_feeds, ["out"], out / "warping_module.onnx", 17)
    finally:
        F.grid_sample = original

    warped = Warp(nets["warping_module"])(feature, kp_driving, kp_source)
    export(nets["spade_generator"], {"feature": warped}, ["image"], out / "spade_generator.onnx", 17)

    feeds = {
        "appearance_feature_extractor.onnx": {"image": image},
        "motion_extractor.onnx": {"image": image},
        "warping_module.onnx": warp_feeds,
        "warping_module_gridsample.onnx": warp_feeds,
        "spade_generator.onnx": {"feature": warped},
    }
    for name, size in [("stitching", 126), ("lip", 65), ("eye", 66)]:
        file = "stitching.onnx" if name == "stitching" else f"retarget_{name}.onnx"
        x = torch.rand(1, size, generator=gen) - 0.5
        export(nets[name], {"input": x}, ["delta"], out / file, 17)
        feeds[file] = {"input": x}
    return feeds


def half_weights(path, feeds):
    """Keeps every float32 weight of the model at `path` as float16, with a
    Cast back to float32 in front of the graph: ONNX Runtime folds the
    casts when it loads the model, so it computes in float32 as before.
    Compares the result with the float32 model and rewrites the file."""
    import onnx
    import onnxruntime as ort
    from onnx import TensorProto, helper, numpy_helper

    feeds = {k: v.numpy() for k, v in feeds.items()}
    ref = ort.InferenceSession(str(path), providers=["CPUExecutionProvider"]).run(None, feeds)
    model = onnx.load(str(path))
    graph = model.graph
    kept, casts = [], []
    for init in graph.initializer:
        values = numpy_helper.to_array(init)
        if init.data_type != TensorProto.FLOAT or values.size < 16:
            kept.append(init)
            continue
        name = init.name + "__float16"
        kept.append(numpy_helper.from_array(values.astype(np.float16), name))
        casts.append(helper.make_node("Cast", [name], [init.name], to=TensorProto.FLOAT, name=name + "_cast"))
    del graph.initializer[:]
    graph.initializer.extend(kept)
    nodes = list(graph.node)
    del graph.node[:]
    graph.node.extend(casts + nodes)
    onnx.checker.check_model(model, full_check=False)
    onnx.save(model, str(path))
    sess = ort.InferenceSession(str(path), providers=["CPUExecutionProvider"])
    got = sess.run(None, feeds)
    worst, angle = 0.0, 0.0
    for o, r, g in zip(sess.get_outputs(), ref, got):
        if o.name in ("pitch", "yaw", "roll"):
            # Logits of 66 bins of 3°: what matters is the angle they give.
            angle = max(angle, float(np.abs(degrees(g) - degrees(r)).max()))
            continue
        span = float(np.abs(r).max()) or 1.0
        worst = max(worst, float(np.abs(g - r).max()) / span)
    angles = f", head angles by {angle:.3f}°" if angle else ""
    print(f"{path.name}: weights as float16, max difference from float32 {worst:.2e} of the range{angles}", flush=True)
    return worst, angle


def float16_copy(path):
    """A copy of the model at `path` that computes in float16, its inputs
    and outputs still float32."""
    import onnx
    from onnxconverter_common import float16

    model = float16.convert_float_to_float16(onnx.load(str(path)), keep_io_types=True)
    dest = path.with_name(path.stem + "_fp16.onnx")
    onnx.save(model, str(dest))
    return dest


def main():
    p = argparse.ArgumentParser()
    p.add_argument("out_dir", type=pathlib.Path)
    p.add_argument("--fp16", action="store_true", help="also write float16 copies for DirectML")
    a = p.parse_args()
    out = a.out_dir
    work = out / ".work"
    work.mkdir(parents=True, exist_ok=True)

    code = clone(work)
    weights = fetch_weights(work, "weights")
    nets = load_networks(code, weights)
    feeds = export_all(nets, out)

    if a.fp16:
        for name in ["warping_module.onnx", "warping_module_gridsample.onnx", "spade_generator.onnx"]:
            print(f"wrote {float16_copy(out / name).name}", flush=True)
    for path in sorted(out.glob("*.onnx")):
        if path.stem.endswith("_fp16"):
            continue
        worst, angle = half_weights(path, feeds[path.name])
        if worst > 5e-3 or angle > 0.5:
            sys.exit(f"{path.name}: float16 weights change the output too much")

    resources = code / "src/utils/resources"
    shutil.copy(resources / "mask_template.png", out / "mask_template.png")
    with open(resources / "lip_array.pkl", "rb") as f:
        lip = np.asarray(pickle.load(f), dtype=np.float32)
    (out / "lip_array.json").write_text(json.dumps([round(float(v), 7) for v in lip.reshape(-1)]))

    sums = []
    print("\nFiles written:")
    for f in sorted(p for p in out.iterdir() if p.is_file() and p.name != "SHA256SUMS"):
        digest = sha256(f)
        sums.append(f"{digest}  {f.name}\n")
        print(f"{digest}  {f.name}  {f.stat().st_size} bytes")
    (out / "SHA256SUMS").write_text("".join(sums))


if __name__ == "__main__":
    main()
