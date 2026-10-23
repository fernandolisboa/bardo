"""Download the two public-domain test recordings and cut the test takes.

    python tools/takes.py OUT_DIR [FFMPEG]

Both are NASA Goddard interviews (GOLD mission resources, 2018), works of
the US federal government and so in the public domain: allowed in the
repository (issue #187, Decided 3). They are not committed because of
their size (140 and 167 MB); this script fetches them by SHA-256.

- take-head-30s.mp4: a seated talking head, 1080p30, 30 s made of two
  answers back to back (one cut at 20.2 s). Speed, face mesh, depth.
- take-gestures-15s.mp4: a standing speaker with loose hair and hand
  gestures, 1080p30, 15.1 s in one shot. Person mask, depth.
"""

import hashlib
import pathlib
import subprocess
import sys
import urllib.request

SOURCES = {
    "head": (
        "https://images-assets.nasa.gov/video/GSFC_20180124_m12825_SarahJones/GSFC_20180124_m12825_SarahJones~orig.mp4",
        "498202e269ecd7d34ce7037faf49dd6e4590240b68226ff395afdf21160285b9",
    ),
    "gestures": (
        "https://images-assets.nasa.gov/video/GSFC_20180124_m12825_Katelynn_Greer/GSFC_20180124_m12825_Katelynn_Greer~orig.mp4",
        "0ff5cf853ec973e1118c223a91f27966d123080a9531fec78599380f001f0279",
    ),
}
ENCODE = ["-c:v", "libopenh264", "-b:v", "12M", "-an"]


def file_sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        while chunk := f.read(1 << 20):
            h.update(chunk)
    return h.hexdigest()


def fetch(url, sha, dest):
    """Downloads next to `dest` and keeps the file only when its SHA-256
    matches, so a broken download never stays behind."""
    if dest.exists() and file_sha256(dest) == sha:
        return
    part = dest.with_name(dest.name + ".part")
    print(f"downloading {url}", flush=True)
    req = urllib.request.Request(url, headers={"User-Agent": "bardo-vision-probe"})
    with urllib.request.urlopen(req) as r, open(part, "wb") as f:
        while chunk := r.read(1 << 20):
            f.write(chunk)
    got = file_sha256(part)
    if got != sha:
        part.unlink()
        sys.exit(f"{url}: SHA-256 {got}, expected {sha}")
    part.replace(dest)


def main(out, ffmpeg="ffmpeg"):
    out = pathlib.Path(out)
    out.mkdir(parents=True, exist_ok=True)
    src = {}
    for name, (url, sha) in SOURCES.items():
        src[name] = out / f"source-{name}.mp4"
        fetch(url, sha, src[name])
    run = lambda *a: subprocess.run([ffmpeg, "-v", "error", "-y", *a], check=True)
    run(
        "-i", str(src["head"]),
        "-filter_complex",
        "[0:v]trim=7.2:27.4,setpts=PTS-STARTPTS[a];[0:v]trim=29.8:39.6,setpts=PTS-STARTPTS[b];"
        "[a][b]concat=n=2:v=1:a=0,fps=30,format=yuv420p[v]",
        "-map", "[v]", *ENCODE, str(out / "take-head-30s.mp4"),
    )
    run("-ss", "7.2", "-to", "22.3", "-i", str(src["gestures"]), "-vf", "fps=30,format=yuv420p", *ENCODE, str(out / "take-gestures-15s.mp4"))


if __name__ == "__main__":
    main(*sys.argv[1:])
