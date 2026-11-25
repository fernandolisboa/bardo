"""Does a mask tell a good cutout from a bad one? (issue #277)

    python tools/confidence.py REPORTS_DIR [--model NAME] [--agree A,B]

Reads the probe's reports (`cut-MODEL-*.json`, one run of `cutout-probe
cut --report`) and the grades given by eye to every cutout
(../grades.toml: good, usable or bad), first counted by the photo's
background. For each statistic of the mask it
prints how well a threshold on it alone separates the bad cutouts from
the rest, then what the rule in `refuse` does: how many bad cutouts it
refuses and how many good ones it refuses with them.

`--agree A,B` asks a second question of the cutouts beside the reports
(`PHOTO.MODEL.png`, which the probe writes with `--out REPORTS_DIR`; the
workflow's results artifact leaves them out for their size): how much do
the masks of models A and B overlap on each photo (intersection over
union), and does a low overlap go with a bad cutout from A?

REPORTS_DIR should hold one run's reports: where two reports cut the
same photo with the same model, the first one read is used and the
script says so.
"""

import argparse
import json
import pathlib
import tomllib

import numpy as np
from PIL import Image

HERE = pathlib.Path(__file__).resolve().parent
GRADES = tomllib.loads((HERE.parent / "grades.toml").read_text())
STATS = ["raw_max", "area", "unsure", "unsure_to_area", "border", "pieces", "largest_piece", "body"]


def refuse(s):
    """The rule the spike proposes: refuse the cutout (fall back to the
    card) when the mask says any of these."""
    return (
        s["area"] < 0.02  # found next to nothing
        or s["area"] > 0.90  # took the whole photo
        or s["border"] > 0.05  # the object is cut by the photo's edge, or background came along
        or s["unsure_to_area"] > 0.25  # much of it undecided
        or s["pieces"] >= 3  # in pieces (two is a product beside its lid; three may be a set)
    )


def agree(reports, a, b):
    """The overlap of two models' masks on every photo, lowest first."""
    out = []
    for path in sorted(reports.glob(f"*.{a}.png")):
        photo = path.name[: -len(f".{a}.png")]
        x = np.asarray(Image.open(path))[..., 3] > 127
        y = np.asarray(Image.open(reports / f"{photo}.{b}.png"))[..., 3] > 127
        out.append((float((x & y).sum() / max((x | y).sum(), 1)), photo))
    return sorted(out)


def rows(reports, model):
    out = []
    for path in sorted(reports.glob("cut-*.json")):
        for m in json.loads(path.read_text())["models"]:
            if model and m["name"] != model:
                continue
            for p in m["photos"]:
                grade = GRADES.get(m["name"], {}).get(p["photo"])
                if grade:
                    out.append((m["name"], p["photo"], grade, p["stats"]))
    return out


def auc(bad, rest):
    """The chance that a bad cutout has a larger value than another one
    (0.5: the statistic says nothing; 0 or 1: it separates them)."""
    if not bad or not rest:
        return float("nan")
    wins = sum((b > r) + 0.5 * (b == r) for b in bad for r in rest)
    return wins / (len(bad) * len(rest))


def main():
    p = argparse.ArgumentParser()
    p.add_argument("reports", type=pathlib.Path)
    p.add_argument("--model")
    p.add_argument("--agree")
    a = p.parse_args()
    if a.agree:
        first, second = a.agree.split(",")
        print(f"overlap  photo          {first}  {second}")
        for iou, photo in agree(a.reports, first, second):
            print(f"{iou:.3f}    {photo:14} {GRADES[first][photo]:7} {GRADES[second][photo]}")
        return
    data = rows(a.reports, a.model)
    seen = {}
    for name, photo, grade, stats in data:
        if (name, photo) in seen:
            print(f"{name}, {photo}: in more than one report; the first is used")
        seen.setdefault((name, photo), (grade, stats))
    data = [(n, ph, g, s) for (n, ph), (g, s) in seen.items()]
    by_grade = {g: sum(1 for d in data if d[2] == g) for g in ("good", "usable", "bad")}
    print(f"{len(data)} graded cutouts: {by_grade}")
    names = sorted({d[0] for d in data})
    print("\ngood / usable / bad by background: plain, then a real scene")
    for name in names:
        cells = []
        for scene in (False, True):
            mine = [d[2] for d in data if d[0] == name and (d[1] in GRADES["scene"]) == scene]
            cells.append(" / ".join(str(mine.count(g)) for g in ("good", "usable", "bad")))
        print(f"{name:34} " + "    ".join(cells))
    print("\nAUC of each statistic (bad against the rest): all models, then " + ", ".join(names))
    for stat in STATS:
        cells = []
        for name in [None] + names:
            mine = [d for d in data if name in (None, d[0])]
            bad = [d[3][stat] for d in mine if d[2] == "bad"]
            rest = [d[3][stat] for d in mine if d[2] != "bad"]
            cells.append(f"{auc(bad, rest):.2f}")
        print(f"{stat:16} " + "  ".join(cells))
    print("\nthe rule, by model: bad refused / bad, usable refused / usable, good refused / good")
    for name in names:
        counts = {}
        for g in ("bad", "usable", "good"):
            mine = [d for d in data if d[0] == name and d[2] == g]
            counts[g] = (sum(refuse(d[3]) for d in mine), len(mine))
        print(f"{name:34} " + ", ".join(f"{g} {r}/{n}" for g, (r, n) in counts.items()))
        for d in data:
            if d[0] == name and ((d[2] == "bad") != refuse(d[3])) and d[2] != "usable":
                print(f"    {'missed' if d[2] == 'bad' else 'refused a good one'}: {d[1]}")


if __name__ == "__main__":
    main()
