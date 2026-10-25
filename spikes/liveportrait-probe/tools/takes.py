"""Download the public-domain test recording and cut the test take.

    python tools/takes.py OUT_DIR [FFMPEG]

The recording is the NASA Goddard interview the vision spike uses
(spikes/vision-probe/tools/takes.py): a work of the US federal government,
so in the public domain. It is not committed because of its size (140 MB);
this script fetches it by SHA-256.

- take-head-20s.mp4: a seated speaker facing the camera, talking, 1080p30,
  20 s in one shot, with its sound, to judge lip sync.
"""

import hashlib
import pathlib
import subprocess
import sys
import urllib.request

URL = "https://images-assets.nasa.gov/video/GSFC_20180124_m12825_SarahJones/GSFC_20180124_m12825_SarahJones~orig.mp4"
SHA256 = "498202e269ecd7d34ce7037faf49dd6e4590240b68226ff395afdf21160285b9"


def file_sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        while chunk := f.read(1 << 20):
            h.update(chunk)
    return h.hexdigest()


def main(out, ffmpeg="ffmpeg"):
    out = pathlib.Path(out)
    out.mkdir(parents=True, exist_ok=True)
    src = out / "source-head.mp4"
    if not (src.exists() and file_sha256(src) == SHA256):
        part = src.with_name(src.name + ".part")
        print(f"downloading {URL}", flush=True)
        req = urllib.request.Request(URL, headers={"User-Agent": "bardo-liveportrait-probe"})
        with urllib.request.urlopen(req) as r, open(part, "wb") as f:
            while chunk := r.read(1 << 20):
                f.write(chunk)
        got = file_sha256(part)
        if got != SHA256:
            part.unlink()
            sys.exit(f"{URL}: SHA-256 {got}, expected {SHA256}")
        part.replace(src)
    # The pinned ffmpeg is LGPL: OpenH264 for the picture, its own AAC.
    encoders = subprocess.run([ffmpeg, "-hide_banner", "-encoders"], capture_output=True, text=True).stdout
    video = ["-c:v", "libx264", "-crf", "16"] if " libx264 " in encoders else ["-c:v", "libopenh264", "-b:v", "12M"]
    subprocess.run(
        [ffmpeg, "-v", "error", "-y", "-ss", "7.2", "-to", "27.2", "-i", str(src),
         "-vf", "fps=30,format=yuv420p", *video, "-c:a", "aac", "-b:a", "160k", str(out / "take-head-20s.mp4")],
        check=True,
    )


if __name__ == "__main__":
    main(*sys.argv[1:])
