"""Write the test fixtures of liveportrait-probe from upstream's own code.

    python tools/fixtures.py LIVEPORTRAIT_CODE tests/fixtures/upstream.json

LIVEPORTRAIT_CODE is upstream's repository at the pinned commit (convert.py
clones it into OUT_DIR/.work/LivePortrait). Every value is computed by
calling upstream's functions, so the Rust port's tests check it against
upstream, not against a second reading of the same code. The one
exception is the relative motion, which upstream computes inline in its
pipeline: its lines are copied here as they stand.
"""

import json
import sys

import numpy as np


def rounded(a):
    return np.round(np.asarray(a, dtype=np.float64), 6).tolist()


def main(code, out):
    sys.path.insert(0, code)
    import cv2
    import torch

    from src.config.crop_config import CropConfig
    from src.config.inference_config import InferenceConfig
    from src.utils.camera import get_rotation_matrix, headpose_pred_to_degree
    from src.utils.crop import (
        average_bbox_lst,
        crop_image,
        crop_image_by_bbox,
        parse_bbox_from_landmark,
        paste_back,
        prepare_paste_back,
    )
    from src.utils.retargeting_utils import calc_lip_close_ratio
    from src.live_portrait_wrapper import LivePortraitWrapper

    rng = np.random.default_rng(7)
    fx = {}

    # Head angles from the 66 bins, and the rotation matrix.
    logits = rng.normal(0, 2, (3, 66)).astype(np.float32)
    degrees = headpose_pred_to_degree(torch.from_numpy(logits)).numpy()
    fx["headpose"] = {"logits": rounded(logits), "degrees": rounded(degrees)}
    angles = torch.tensor([[12.0], [-25.0], [7.5]])
    rot = get_rotation_matrix(angles[0], angles[1], angles[2])[0].numpy()
    fx["rotation"] = {"pitch": 12.0, "yaw": -25.0, "roll": 7.5, "matrix": rounded(rot)}

    # Keypoints in the source's pose: s * (kp R + exp) + t, with tz dropped.
    info = {
        "pitch": torch.tensor([[4.0]]),
        "yaw": torch.tensor([[-9.0]]),
        "roll": torch.tensor([[2.0]]),
        "t": torch.tensor([[0.01, -0.02, 0.3]]),
        "exp": torch.from_numpy(rng.normal(0, 0.02, (1, 21, 3)).astype(np.float32)),
        "scale": torch.tensor([[1.4]]),
        "kp": torch.from_numpy(rng.normal(0, 0.2, (1, 21, 3)).astype(np.float32)),
    }
    x_s = LivePortraitWrapper.transform_keypoint(None, dict(info))
    fx["transform_keypoint"] = {
        "pitch": 4.0, "yaw": -9.0, "roll": 2.0,
        "t": rounded(info["t"][0]), "exp": rounded(info["exp"][0].reshape(-1)),
        "scale": 1.4, "kp": rounded(info["kp"][0].reshape(-1)),
        "x_s": rounded(x_s[0].reshape(-1)),
    }

    # One driving frame against the first, as the pipeline composes it for
    # a still source and a driving video ("pose-friendly", relative motion,
    # animation region "all"), before stitching.
    def frame_info(pitch, yaw, roll, scale, t, exp):
        return {
            "R": get_rotation_matrix(torch.tensor([[pitch]]), torch.tensor([[yaw]]), torch.tensor([[roll]])),
            "scale": torch.tensor([[scale]]),
            "t": torch.tensor([t]),
            "exp": torch.from_numpy(np.asarray(exp, dtype=np.float32).reshape(1, 21, 3)),
        }

    d0 = frame_info(2.0, 5.0, -1.0, 1.5, [0.02, 0.01, 0.4], rng.normal(0, 0.02, 63))
    di = frame_info(-6.0, 14.0, 3.0, 1.6, [0.05, -0.01, 0.2], rng.normal(0, 0.02, 63))
    r_s = get_rotation_matrix(info["pitch"], info["yaw"], info["roll"])
    r_new = (di["R"] @ d0["R"].permute(0, 2, 1)) @ r_s
    delta_new = info["exp"] + (di["exp"] - d0["exp"])
    scale_new = info["scale"] * (di["scale"] / d0["scale"])
    t_new = info["t"] + (di["t"] - d0["t"])
    t_new[..., 2].fill_(0)
    x_d = scale_new * (info["kp"] @ r_new + delta_new) + t_new
    fx["relative_motion"] = {
        "first": {"pitch": 2.0, "yaw": 5.0, "roll": -1.0, "scale": 1.5, "t": rounded(d0["t"][0]), "exp": rounded(d0["exp"].reshape(-1))},
        "frame": {"pitch": -6.0, "yaw": 14.0, "roll": 3.0, "scale": 1.6, "t": rounded(di["t"][0]), "exp": rounded(di["exp"].reshape(-1))},
        "x_d": rounded(x_d[0].reshape(-1)),
    }

    # Stitching: the network's 65 outputs move the 63 coordinates and then x
    # and y of every point.
    class Stitch:
        stitching_retargeting_module = {"stitching": lambda feat: torch.from_numpy(delta)}

    delta = rng.normal(0, 0.01, (1, 65)).astype(np.float32)
    kp_d = torch.from_numpy(rng.normal(0, 0.2, (1, 21, 3)).astype(np.float32))
    stitched = LivePortraitWrapper.stitching(
        type("W", (), {"stitching_retargeting_module": Stitch.stitching_retargeting_module, "stitch": lambda self, a, b: torch.from_numpy(delta)})(),
        x_s, kp_d,
    )
    fx["stitching"] = {"kp_driving": rounded(kp_d[0].reshape(-1)), "delta": rounded(delta[0]), "stitched": rounded(stitched[0].reshape(-1))}

    # The lip-open ratio LivePortrait reads from its 203 landmarks.
    lmk = rng.uniform(0, 512, (203, 2)).astype(np.float32)
    fx["lip_ratio"] = {"points": rounded(lmk), "ratio": float(calc_lip_close_ratio(lmk[None])[0, 0])}

    # The source crop: a turned face of 203 points; the crop's transform.
    crop = CropConfig()
    face = rng.normal(0, 1, (203, 2)) * [60, 80]
    turn = np.deg2rad(12)
    face = face @ np.array([[np.cos(turn), np.sin(turn)], [-np.sin(turn), np.cos(turn)]]) + [700, 420]
    face = face.astype(np.float32)
    img = rng.integers(0, 256, (900, 1280, 3), dtype=np.uint8)
    ret = crop_image(img, face, dsize=crop.dsize, scale=crop.scale, vx_ratio=crop.vx_ratio, vy_ratio=crop.vy_ratio, flag_do_rot=True)
    fx["source_crop"] = {
        "points": rounded(face),
        "dsize": crop.dsize, "scale": crop.scale, "vx_ratio": crop.vx_ratio, "vy_ratio": crop.vy_ratio,
        "m_o2c": rounded(ret["M_o2c"][:2]),
    }

    # The driving crop: the axis-aligned box of each frame, averaged.
    boxes = []
    for k in range(3):
        moved = (face + [k * 9.0, -k * 4.0]).astype(np.float32)
        b = parse_bbox_from_landmark(moved, scale=crop.scale_crop_driving_video, vx_ratio_crop_driving_video=crop.vx_ratio_crop_driving_video, vy_ratio=crop.vy_ratio_crop_driving_video)["bbox"]
        boxes.append([b[0, 0], b[0, 1], b[2, 0], b[2, 1]])
    box = average_bbox_lst(boxes)
    by_box = crop_image_by_bbox(img, box, lmk=face, dsize=512, flag_rot=False, borderValue=(0, 0, 0))
    fx["driving_crop"] = {
        "offsets": [[k * 9.0, -k * 4.0] for k in range(3)],
        "scale": crop.scale_crop_driving_video, "vy_ratio": crop.vy_ratio_crop_driving_video,
        "box": rounded(box), "m_o2c": rounded(by_box["M_o2c"][:2]),
    }

    # Pixels: an affine warp as cv2.warpAffine does it (bilinear, black
    # outside), the 2:1 area resize, and the paste-back blend, on a small
    # image.
    small = rng.integers(0, 256, (24, 32, 3), dtype=np.uint8)
    m = np.array([[0.9, 0.15, 3.2], [-0.12, 1.1, -1.7]], dtype=np.float32)
    warped = cv2.warpAffine(small, m, dsize=(20, 18), flags=cv2.INTER_LINEAR)
    half = cv2.resize(small, (16, 12), interpolation=cv2.INTER_AREA)
    mask = rng.integers(0, 256, (18, 20, 3), dtype=np.uint8)
    crop_img = rng.integers(0, 256, (18, 20, 3), dtype=np.uint8)
    m_c2o = np.vstack([np.array([[1.2, -0.1, 4.0], [0.1, 1.2, 2.0]]), [0, 0, 1]]).astype(np.float32)
    mask_ori = prepare_paste_back(mask, m_c2o, dsize=(32, 24))
    pasted = paste_back(crop_img, m_c2o, small, mask_ori)
    fx["pixels"] = {
        "image": small.reshape(-1).tolist(), "width": 32, "height": 24,
        "affine": rounded(m), "warped": warped.reshape(-1).tolist(), "warped_width": 20, "warped_height": 18,
        "half": half.reshape(-1).tolist(),
        "mask": mask.reshape(-1).tolist(), "crop": crop_img.reshape(-1).tolist(), "m_c2o": rounded(m_c2o[:2]),
        "pasted": pasted.reshape(-1).tolist(),
    }

    cfg = InferenceConfig()
    fx["config"] = {"lip_normalize_threshold": cfg.lip_normalize_threshold, "driving_multiplier": cfg.driving_multiplier}

    with open(out, "w") as f:
        json.dump(fx, f, separators=(",", ":"))
        f.write("\n")


if __name__ == "__main__":
    main(*sys.argv[1:])
