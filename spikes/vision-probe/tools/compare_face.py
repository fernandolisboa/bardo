"""Compare vision-probe's face_landmarks.json with MediaPipe's own Face
Landmarker (the Python task, VIDEO mode, same .task bundle) on the same video.

    python compare_face.py VIDEO face_landmarker.task PROBE_JSON

Prints the mean and 95th-percentile landmark distance in pixels of the
frame, the blendshape mean absolute difference, and frames where only one
side found a face.
"""

import json
import sys

import cv2
import mediapipe as mp
import numpy as np
from mediapipe.tasks.python import BaseOptions, vision


def main(video, task, probe_json):
    probe = json.load(open(probe_json))["frames"]
    opts = vision.FaceLandmarkerOptions(
        base_options=BaseOptions(model_asset_path=task),
        running_mode=vision.RunningMode.VIDEO,
        output_face_blendshapes=True,
        num_faces=1,
    )
    lm = vision.FaceLandmarker.create_from_options(opts)
    cap = cv2.VideoCapture(video)
    fps = cap.get(cv2.CAP_PROP_FPS)
    w, h = cap.get(3), cap.get(4)
    dists, blend, only_one, i = [], [], 0, 0
    while i < len(probe):
        ok, frame = cap.read()
        if not ok:
            break
        img = mp.Image(image_format=mp.ImageFormat.SRGB, data=cv2.cvtColor(frame, cv2.COLOR_BGR2RGB))
        ref = lm.detect_for_video(img, int(round(i * 1000 / fps)))
        ours = probe[i]["faces"]
        if bool(ref.face_landmarks) != bool(ours):
            only_one += 1
        elif ours:
            a = np.array([[p.x * w, p.y * h] for p in ref.face_landmarks[0]])
            b = np.array(ours[0]["landmarks"])[:, :2] * [w, h]
            dists.append(np.linalg.norm(a - b, axis=1))
            rb = {c.category_name: c.score for c in ref.face_blendshapes[0]}
            blend.append(np.mean([abs(rb[k] - v) for k, v in ours[0]["blendshapes"].items()]))
        i += 1
    d = np.concatenate(dists)
    face_px = np.median([np.ptp(np.array(f["faces"][0]["landmarks"])[:, 1]) * h for f in probe if f["faces"]])
    print(f"frames {i}, compared {len(dists)}, face found by one side only {only_one}")
    print(f"landmark distance px: mean {d.mean():.2f}, p95 {np.percentile(d, 95):.2f}, max {d.max():.2f} (face height {face_px:.0f} px)")
    print(f"blendshape mean abs diff {np.mean(blend):.4f}")


if __name__ == "__main__":
    main(*sys.argv[1:])
