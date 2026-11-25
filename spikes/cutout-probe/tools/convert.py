"""Download the pinned salient-object models of the product-cutout spike
(issue #277) and turn them into the ONNX files the probe runs.

    python tools/convert.py OUT_DIR [--deformconv]

Every download is checked against its SHA-256 and every upstream
repository is checked out at its pinned commit (../models.toml). Each
checkpoint is loaded into its upstream model code, exported with
torch.onnx at a fixed input size and compared with PyTorch on the same
input. Each file takes a normalized photo (`image`, 1 x 3 x H x W) and
gives an alpha before any stretch (`mask`, 1 x 1 x H x W, 0..1). Its
weights are then kept as float16 and turned back into float32 when the
model loads, as crates/vision/tools/convert.py does, and the result is
compared with the float32 file on a test photo. Writes the files the manifest's [[model]]
entries list, SHA256SUMS, and a copy of the manifest for the probe.

BiRefNet's decoder uses deformable convolutions. ONNX has them as
DeformConv (opset 19), which ONNX Runtime runs on the CPU from 1.25 on;
the runtime pinned by ADR-0024 is 1.24.2. So the convolutions are written
here with grid_sample, a kernel tap at a time, which exports to GridSample
(opset 16) and Conv and is checked against torchvision's own
deform_conv2d. `--deformconv` also writes `*_deformconv.onnx` with the
native operator, for comparing on a newer runtime; those files are not in
the manifest.
"""

import argparse
import hashlib
import pathlib
import shutil
import subprocess
import sys
import tomllib
import urllib.request

import numpy as np

HERE = pathlib.Path(__file__).resolve().parent
MANIFEST_PATH = HERE.parent / "models.toml"
MANIFEST = tomllib.loads(MANIFEST_PATH.read_text())
SOURCES = MANIFEST["sources"]
OPSET = 17


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def fetch(entry, cache):
    """The path of a source's download, from the cache when there."""
    url, want = entry["url"], entry["sha256"]
    path = cache / want
    if path.exists() and sha256(path.read_bytes()) == want:
        return path
    print(f"downloading {url}", flush=True)
    req = urllib.request.Request(url, headers={"User-Agent": "bardo-cutout-spike"})
    with urllib.request.urlopen(req) as r:
        data = r.read()
    got = sha256(data)
    if got != want:
        sys.exit(f"{url}: SHA-256 {got}, expected {want}")
    path.write_bytes(data)
    return path


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


def forget(*names):
    """Drops imported upstream modules: two repositories both have a
    `models` package."""
    for key in list(sys.modules):
        if any(key == n or key.startswith(n + ".") for n in names):
            del sys.modules[key]


def deform_conv2d_taps(x, offset, weight, bias, padding, mask):
    """torchvision.ops.deform_conv2d for stride 1, dilation 1 and one
    group, a kernel tap at a time: each tap samples the input where the
    offsets send it (bilinear, zero outside, as torchvision does), weighs
    it by the mask and goes through that tap's 1 x 1 convolution."""
    import torch
    import torch.nn.functional as F

    _, _, h, w = x.shape
    _, _, kh, kw = weight.shape
    ph, pw = (padding, padding) if isinstance(padding, int) else padding
    if (h + 2 * ph - kh + 1, w + 2 * pw - kw + 1) != (h, w):
        raise ValueError("deform_conv2d_taps keeps the input's size only")
    ys = torch.arange(h, dtype=x.dtype).view(1, h, 1).expand(1, h, w)
    xs = torch.arange(w, dtype=x.dtype).view(1, 1, w).expand(1, h, w)
    out = None
    for i in range(kh):
        for j in range(kw):
            k = i * kw + j
            py = ys + (i - ph) + offset[:, 2 * k]
            px = xs + (j - pw) + offset[:, 2 * k + 1]
            grid = torch.stack((px * (2.0 / (w - 1)) - 1.0, py * (2.0 / (h - 1)) - 1.0), dim=-1)
            tap = F.grid_sample(x, grid, mode="bilinear", padding_mode="zeros", align_corners=True)
            tap = F.conv2d(tap * mask[:, k : k + 1], weight[:, :, i : i + 1, j : j + 1])
            out = tap if out is None else out + tap
    if bias is not None:
        out = out + bias.view(1, -1, 1, 1)
    return out


def check_taps():
    """The rewrite against torchvision's operator, for the kernel sizes
    BiRefNet uses."""
    import torch
    from torchvision.ops import deform_conv2d

    g = torch.Generator().manual_seed(0)
    worst = 0.0
    for k, p in [(1, 0), (3, 1), (7, 3)]:
        x = torch.randn(1, 8, 20, 28, generator=g)
        offset = torch.randn(1, 2 * k * k, 20, 28, generator=g) * 3
        mask = torch.rand(1, k * k, 20, 28, generator=g) * 2
        weight = torch.randn(5, 8, k, k, generator=g)
        bias = torch.randn(5, generator=g)
        ref = deform_conv2d(x, offset, weight, bias, padding=p, mask=mask)
        got = deform_conv2d_taps(x, offset, weight, bias, p, mask)
        worst = max(worst, float((ref - got).abs().max() / ref.abs().max()))
    print(f"deform_conv2d by taps: max difference from torchvision {worst:.2e} of the range")
    if worst > 1e-4:
        sys.exit("the deform_conv2d rewrite does not match torchvision")


def register_deformconv():
    """torchvision's deform_conv2d as ONNX DeformConv (opset 19)."""
    from torch.onnx import register_custom_op_symbolic
    from torch.onnx import symbolic_helper as sh

    @sh.parse_args("v", "v", "v", "v", "v", "i", "i", "i", "i", "i", "i", "i", "i", "b")
    def deform(g, x, weight, offset, mask, bias, sh_, sw, ph, pw, dh, dw, groups, offset_groups, use_mask):
        if not use_mask:
            raise ValueError("deform_conv2d without a mask is not exported")
        return g.op("DeformConv", x, weight, offset, bias, mask, strides_i=[sh_, sw], pads_i=[ph, pw, ph, pw], dilations_i=[dh, dw], group_i=groups, offset_group_i=offset_groups)

    register_custom_op_symbolic("torchvision::deform_conv2d", deform, 19)


def export(model, x, out, opset=OPSET):
    """Exports `model` at `x`'s size and compares the file with PyTorch."""
    import onnxruntime as ort
    import torch

    with torch.no_grad():
        ref = model(x).numpy()
        torch.onnx.export(model, (x,), str(out), input_names=["image"], output_names=["mask"], opset_version=opset, dynamo=False)
    got = ort.InferenceSession(str(out), providers=["CPUExecutionProvider"]).run(None, {"image": x.numpy()})[0]
    if got.shape != (1, 1, x.shape[2], x.shape[3]):
        sys.exit(f"{out.name}: output {got.shape}")
    worst = float(np.abs(got - ref).max())
    print(f"{out.name}: max difference from PyTorch {worst:.2e} (the mask runs 0..1)", flush=True)
    if worst > 1e-3:
        sys.exit(f"{out.name} does not match PyTorch")


def wrap(model, pick):
    import torch

    class Mask(torch.nn.Module):
        def __init__(self):
            super().__init__()
            self.model = model

        def forward(self, x):
            return pick(self.model(x))

    return Mask().eval()


def sample(h, w):
    import torch

    return torch.randn(1, 3, h, w, generator=torch.Generator().manual_seed(0))


def export_u2net(key, entry, work, out):
    import torch

    code = checkout(SOURCES[key]["code"], work)
    sys.path.insert(0, str(code / "model"))
    forget("u2net")
    import u2net

    net = {"u2net": u2net.U2NET, "u2netp": u2net.U2NETP}[key](3, 1)
    net.load_state_dict(torch.load(fetch(SOURCES[key], work), map_location="cpu"))
    sys.path.pop(0)
    # The first of the seven outputs is the fused map, already a sigmoid.
    export(wrap(net.eval(), lambda o: o[0]), sample(*entry["input"]), out)


def export_isnet(key, entry, work, out):
    import torch

    code = checkout(SOURCES[key]["code"], work)
    sys.path.insert(0, str(code / "IS-Net"))
    forget("models")
    from models.isnet import ISNetDIS

    net = ISNetDIS()
    net.load_state_dict(torch.load(fetch(SOURCES[key], work), map_location="cpu"))
    sys.path.pop(0)
    forget("models")
    # ([maps], [features]); the first map is the finest, already a sigmoid.
    export(wrap(net.eval(), lambda o: o[0][0]), sample(*entry["input"]), out)


def export_birefnet(key, entry, work, out, deformconv):
    import torch
    from safetensors.torch import load_file

    upstream = checkout(SOURCES[key]["code"], work)
    # Upstream picks the task and the backbone by editing config.py, which
    # every module reads on import; a patched copy does the same.
    task = "Matting" if "matting" in key else "General"
    tasks = ["DIS5K", "COD", "HRSOD", "General", "General-2K", "Matting"]
    code = work / f"birefnet-{key}"
    if code.exists():
        shutil.rmtree(code)
    shutil.copytree(upstream, code, ignore=shutil.ignore_patterns(".git"))
    config = (code / "config.py").read_text()
    task_line = "self.task = ['DIS5K', 'COD', 'HRSOD', 'General', 'General-2K', 'Matting'][0]"
    backbone_line = "'dino_v3_b', 'dino_v3_s_plus', 'dino_v3_s',\n        ][3]"
    if config.count(task_line) != 1 or config.count(backbone_line) != 1:
        sys.exit("BiRefNet's config.py is not laid out as at the pinned commit")
    config = config.replace(task_line, task_line[:-3] + f"[{tasks.index(task)}]")
    # Index 6 of the list is swin_v1_t, the lite models' backbone.
    config = config.replace(backbone_line, backbone_line[:-3] + "[6]")
    (code / "config.py").write_text(config)

    sys.path.insert(0, str(code))
    forget("config", "dataset", "utils", "models", "image_proc")
    import models.modules.deform_conv as deform_conv
    from models.birefnet import BiRefNet

    net = BiRefNet(bb_pretrained=False)
    if net.config.bb != "swin_v1_t" or net.config.task != task:
        sys.exit(f"BiRefNet built with {net.config.bb} for {net.config.task}")
    state = {k: v.float() for k, v in load_file(fetch(SOURCES[key], work)).items()}
    net.load_state_dict(state, strict=True)
    # The last output is the finest map, as logits.
    model = wrap(net.eval(), lambda o: torch.sigmoid(o[-1]))
    x = sample(*entry["input"])

    upstream_forward = deform_conv.DeformableConv2d.forward
    if deformconv:
        register_deformconv()
        export(model, x, out.with_name(out.stem + "_deformconv.onnx"), opset=19)

    def by_taps(self, x):
        offset = self.offset_conv(x)
        modulator = 2.0 * torch.sigmoid(self.modulator_conv(x))
        return deform_conv2d_taps(x, offset, self.regular_conv.weight, self.regular_conv.bias, self.padding, modulator)

    with torch.no_grad():
        ref = model(x)
        deform_conv.DeformableConv2d.forward = by_taps
        worst = float((model(x) - ref).abs().max())
    print(f"{out.name}: deform_conv2d by taps, max difference from upstream's forward {worst:.2e}")
    if worst > 1e-3:
        sys.exit(f"{out.name}: the rewrite changes the mask")
    export(model, x, out)
    deform_conv.DeformableConv2d.forward = upstream_forward
    sys.path.pop(0)
    forget("config", "dataset", "utils", "models", "image_proc")


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
    worst = max(float(np.abs(g - r).max()) for r, g in zip(ref, got))
    mean = max(float(np.abs(g - r).mean()) for r, g in zip(ref, got))
    print(f"{path.name}: {halved} weights as float16, difference from float32 on a test photo: mean {mean:.2e}, max {worst:.2e} (the mask runs 0..1)", flush=True)
    # An alpha is kept in 8 bits, whose step is 4e-3. Single pixels on a
    # steep edge move more than the mask does; a wrong conversion moves
    # everything.
    if mean > 1e-3 or worst > 0.1:
        sys.exit(f"{path.name}: float16 weights change the mask too much")


def photo_input(entry):
    """A test photo as the probe prepares it for this model: what the
    float16 weights are checked on, since a mask of noise says little."""
    from PIL import Image

    h, w = entry["input"]
    photo = Image.open(HERE.parent / "photos" / "plain-2.jpg").convert("RGB").resize((w, h), Image.BILINEAR)
    x = np.asarray(photo, dtype=np.float32)
    x = x / (x.max() if entry["scale"] == "max" else 255.0)
    x = (x - np.array(entry["mean"], dtype=np.float32)) / np.array(entry["std"], dtype=np.float32)
    return np.ascontiguousarray(x.transpose(2, 0, 1)[None])


def main():
    p = argparse.ArgumentParser()
    p.add_argument("out_dir", type=pathlib.Path)
    p.add_argument("--deformconv", action="store_true", help="also write BiRefNet with ONNX's own DeformConv")
    a = p.parse_args()
    out = a.out_dir
    work = out / ".work"
    work.mkdir(parents=True, exist_ok=True)

    check_taps()
    exporters = {
        "u2net": export_u2net,
        "u2netp": export_u2net,
        "isnet_general_use": export_isnet,
    }
    wanted = []
    for entry in MANIFEST["model"]:
        key, path = entry["from"], out / entry["file"]
        if key.startswith("birefnet"):
            export_birefnet(key, entry, work, path, a.deformconv)
        else:
            exporters[key](key, entry, work, path)
        wanted.append(path)
    extra = sorted(out.glob("*_deformconv.onnx"))

    for path in wanted + extra:
        entry = next(e for e in MANIFEST["model"] if path.name.startswith(e["file"][: -len(".onnx")]))
        half_weights(path, {"image": photo_input(entry)})

    sums = []
    print("\nSHA-256 of the files written:")
    for path in wanted + extra:
        digest = sha256(path.read_bytes())
        sums.append(f"{digest}  {path.name}\n")
        print(f"{digest}  {path.name}  {path.stat().st_size} bytes")
    (out / "SHA256SUMS").write_text("".join(sums))
    shutil.copyfile(MANIFEST_PATH, out / "models.toml")


if __name__ == "__main__":
    main()
