"""Compare the converted selfie multiclass segmenter with MediaPipe's own
Image Segmenter (the Python task, same .tflite) on frames of a video, with
the frame stretched to 256x256 (as vision-probe does) and letterboxed.

    python compare_mask.py VIDEO selfie_multiclass_256x256.onnx \
        selfie_multiclass_256x256.tflite [FRAMES]

Prints, for each way of fitting the frame, the mean person-mask IoU against
MediaPipe over FRAMES evenly spaced frames (default 10).
"""

import sys

import cv2
import mediapipe as mp
import numpy as np
import onnxruntime as ort
from mediapipe.tasks.python import BaseOptions, vision


def softmax(y):
    y = np.exp(y - y.max(-1, keepdims=True))
    return y / y.sum(-1, keepdims=True)


def person_iou(a, b):
    # Class 0 is the background.
    pa, pb = a[..., 0] < 0.5, b[..., 0] < 0.5
    return (pa & pb).sum() / max((pa | pb).sum(), 1)


def onnx_probs(session, rgb, letterbox):
    h, w = rgb.shape[:2]
    if letterbox:
        ch = int(round(h * 256 / w))
        pad = (256 - ch) // 2
        img = np.zeros((256, 256, 3), np.float32)
        img[pad : pad + ch] = cv2.resize(rgb, (256, ch), interpolation=cv2.INTER_LINEAR)
    else:
        img = cv2.resize(rgb, (256, 256), interpolation=cv2.INTER_LINEAR).astype(np.float32)
    x = (img / 127.5 - 1)[None].astype(np.float32)
    y = softmax(session.run(None, {session.get_inputs()[0].name: x})[0][0])
    if letterbox:
        y = y[pad : pad + ch]
    return cv2.resize(y, (w, h), interpolation=cv2.INTER_LINEAR)


def main(video, onnx_path, tflite_path, frames=10):
    session = ort.InferenceSession(onnx_path)
    opts = vision.ImageSegmenterOptions(
        base_options=BaseOptions(model_asset_path=tflite_path),
        output_confidence_masks=True,
    )
    segmenter = vision.ImageSegmenter.create_from_options(opts)
    cap = cv2.VideoCapture(video)
    total = int(cap.get(cv2.CAP_PROP_FRAME_COUNT))
    ious = {"stretch": [], "letterbox": []}
    for i in np.linspace(0, total - 1, frames).astype(int):
        cap.set(cv2.CAP_PROP_POS_FRAMES, int(i))
        ok, bgr = cap.read()
        if not ok:
            continue
        rgb = cv2.cvtColor(bgr, cv2.COLOR_BGR2RGB)
        res = segmenter.segment(mp.Image(image_format=mp.ImageFormat.SRGB, data=rgb))
        ref = np.stack(
            [m.numpy_view().reshape(rgb.shape[:2]) for m in res.confidence_masks], -1
        )
        for name in ious:
            ious[name].append(person_iou(onnx_probs(session, rgb, name == "letterbox"), ref))
    for name, values in ious.items():
        print(f"{name}: mean person IoU {np.mean(values):.3f} over {len(values)} frames")


if __name__ == "__main__":
    main(*sys.argv[1:4], *(int(a) for a in sys.argv[4:5]))
