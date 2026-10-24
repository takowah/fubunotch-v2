# The five members as tiny SD chibis, 24 x 26, one shared frame: a boxy head
# of hair with the face a small window, line eyes, a tiny sitting body, a plum
# line around each part (its own layer), 15 animation frames. Each member
# changes only what makes her her: hair colours and shape, ears or a hat,
# the tail, the outfit's colours, a few accent pixels.
# Fubuki is the one the owner approved on 24 Oct 2026 (pixel-art/fubuki-tiny, kept
# pixel for pixel here); the others follow her. pixel-art-studio, study 007.
#   python build.py   -> sheet.png (every member, every frame), idle-<member>.gif
import os
import sys

sys.path.insert(0, os.path.expanduser("~/.claude/skills/pixel-art-studio/scripts"))
from pixelstudio import Sprite  # noqa: E402

W, H = 24, 26
PLUM = "#5d2d60"
BASE = {"k": PLUM, "s": "#ffd2c2", "p": "#ffa3b1", "c": "#fff6e8", "W": "#ffffff"}

FRAMES = ["idle", "idle2", "blink", "twitch", "wag1", "wag2", "happy", "sleep",
          "walk1", "walk2", "walk3", "walk4", "climb1", "climb2", "sit"]

# H hair, h its shade; T top, C collar, B bow, K skirt; E ear inside; Y tail, y its shade;
# then accents per member.
MEMBERS = {
    "fubuki": {"H": "#ffffff", "h": "#cfd6e6", "T": "#ffffff", "C": PLUM, "B": "#7fcfff", "K": PLUM,
               "E": "#cfd6e6", "Y": "#ffffff", "y": "#cfd6e6", "ears": "fox", "tail": "fox", "ahoge": True},
    "mio": {"H": "#3b3449", "h": "#262031", "T": "#2c2838", "C": "#ffffff", "B": "#e0384e", "K": "#2c2838",
            "E": "#e9e6ee", "Y": "#3b3449", "y": "#e9e6ee", "R": "#e0384e", "Z": "#f6c63a",
            "ears": "wolf", "tail": "wolf"},
    "towa": {"H": "#b98ae8", "h": "#8c62c8", "P": "#f290c8", "T": "#ffffff", "C": "#2c2838", "B": "#f0559c",
             "K": "#2c2838", "D": "#2c2838", "X": "#f0559c", "Y": "#2c2838", "y": "#2c2838",
             "hat": "horned cap", "tail": "devil", "twintails": True},
    "azki": {"H": "#5a3242", "h": "#3d2230", "P": "#ef4f8a", "T": "#2c2838", "C": "#ffffff", "B": "#ef4f8a",
             "K": "#2c2838", "Z": "#f2cc63", "X": "#ef4f8a", "hat": "headband"},
    "suisei": {"H": "#7cb6f2", "h": "#4f88d4", "T": "#ffffff", "C": "#2a3566", "B": "#9fd4ff", "K": "#2a3566",
               "D": "#2a3566", "d": "#1d2550", "Z": "#f2c94c", "hat": "beret", "ponytail": True},
}


def rgb(h):
    h = h.lstrip("#")
    return (int(h[0:2], 16), int(h[2:4], 16), int(h[4:6], 16))


class Pen:
    """Spans and points in a member's colours."""

    def __init__(self, s, member):
        self.s = s
        self.col = {**BASE, **{k: v for k, v in MEMBERS[member].items() if isinstance(v, str) and v.startswith("#")}}

    def spans(self, rows, key):
        for y, runs in rows.items():
            for x0, x1 in runs:
                for x in range(x0, x1 + 1):
                    self.s.px(x, y, self.col[key])

    def pts(self, key, points):
        for x, y in points:
            self.s.px(x, y, self.col[key])

    def shade(self, key, shadow):
        """`shadow` where `key` turns away from a top-left light (its right/lower edge)."""
        s, c = self.s, rgb(self.col[key])
        hits = [(x, y) for y in range(H) for x in range(W)
                if (p := s.get(x, y)) and p[:3] == c and (not s.get(x + 1, y) or not s.get(x, y + 1))]
        for x, y in hits:
            s.px(x, y, self.col[shadow])


def tail(p, kind, frame):
    s = p.s
    if kind in ("fox", "wolf"):
        rows = {11: [(21, 21)], 12: [(20, 22)], 13: [(19, 22)], 14: [(19, 22)], 15: [(20, 22)], 16: [(19, 22)], 17: [(18, 22)],
                18: [(17, 22)], 19: [(16, 22)], 20: [(15, 21)], 21: [(15, 21)], 22: [(15, 20)], 23: [(15, 20)], 24: [(16, 18)]}
        p.spans(rows, "Y")
        if kind == "wolf":                      # a light tip
            p.spans({11: [(21, 21)], 12: [(20, 22)], 13: [(19, 22)]}, "y")
    elif kind == "devil":                       # thin, a pink spade at the end
        p.pts("Y", [(15, 23), (16, 23), (17, 22), (18, 21), (19, 20), (20, 19), (20, 18), (20, 17)])
        p.spans({14: [(20, 20)], 15: [(19, 21)], 16: [(19, 21)]}, "X")
    else:
        return
    if frame in ("wag1", "walk1", "walk3"):
        s.shift(1, 0)
    elif frame == "wag2":
        s.shift(-1, 1)
    if kind != "devil":
        p.shade("Y", "y" if kind == "fox" else "h")
    s.outline(PLUM)


def body(p, frame):
    s = p.s
    p.spans({18: [(6, 12)]}, "C")
    p.spans({19: [(5, 13)], 20: [(5, 13)], 21: [(5, 13)]}, "T")
    p.pts("B", [(8, 19), (9, 19), (10, 19), (9, 20)])
    p.spans({22: [(5, 13)]}, "K")
    legs = {"walk1": [(5, 6), (12, 13)], "walk3": [(5, 6), (12, 13)], "walk2": [(6, 7), (11, 12)], "walk4": [(6, 7), (11, 12)],
            "sit": [(4, 6), (12, 14)]}.get(frame, [(6, 7), (11, 12)])
    if frame in ("walk1", "walk3"):
        lifted = legs[0] if frame == "walk1" else legs[1]
        legs = [l for l in legs if l != lifted]
    p.spans({23: legs}, "s")
    if p.col["T"] == "#ffffff":
        p.shade("T", "h") if p.col["h"] != "#cfd6e6" else p.shade("T", "h")
    s.outline(PLUM)


def paws(p, frame):
    rows = {"happy": {15: [(3, 4), (14, 15)], 16: [(3, 4), (14, 15)]},
            "climb1": {7: [(1, 2)], 8: [(1, 2)], 10: [(16, 17)], 11: [(16, 17)]},
            "climb2": {10: [(1, 2)], 11: [(1, 2)], 7: [(16, 17)], 8: [(16, 17)]}}.get(frame, {20: [(3, 4), (14, 15)], 21: [(3, 4), (14, 15)]})
    p.spans(rows, "c")
    p.s.outline(PLUM)


def ears(p, kind, frame):
    s = p.s
    if kind is None:
        return False
    # ours-left ear: upright, a plum tip (fox) or all hair (wolf)
    tip = "k" if kind == "fox" else "H"
    p.spans({1: [(4, 4)], 2: [(3, 4)], 3: [(4, 4)]}, tip)
    p.spans({3: [(3, 3), (5, 5)], 4: [(2, 3), (5, 5)], 5: [(2, 2), (5, 6)], 6: [(2, 7)]}, "H" if kind == "wolf" else "W")
    p.pts("E", [(4, 4), (3, 5), (4, 5)])
    if kind == "wolf":
        # the other one upright too, mirrored about the head (x -> 20 - x)
        flick = 1 if frame == "twitch" else 0
        p.spans({1 + flick: [(16, 16)], 2 + flick: [(16, 17)], 3 + flick: [(16, 16)]}, "H")
        p.spans({3 + flick: [(15, 15), (17, 17)], 4 + flick: [(15, 15), (17, 18)], 5 + flick: [(14, 15), (18, 18)], 6: [(13, 18)]}, "H")
        p.pts("E", [(16, 4 + flick), (16, 5 + flick), (17, 5 + flick)])
    elif frame == "twitch":
        p.spans({1: [(19, 19)], 2: [(18, 19)]}, "k")
        p.spans({3: [(17, 18)], 4: [(16, 18)], 5: [(15, 17)], 6: [(15, 17)]}, "W")
        p.pts("E", [(17, 4), (16, 5)])
    else:
        p.spans({3: [(20, 21)], 4: [(20, 20)]}, "k")
        p.spans({3: [(18, 19)], 4: [(16, 16), (19, 19)], 5: [(15, 15), (17, 18)], 6: [(15, 17)]}, "W")
        p.pts("E", [(17, 4), (18, 4), (16, 5)])
    s.outline(PLUM)
    return True


def sidetails(p, m):
    """Twin tails or a side ponytail: their own layer behind the head, so a
    line parts them from it."""
    s = p.s
    if m.get("twintails"):                      # down past the shoulders, pink at the ends
        p.spans({8: [(1, 2), (19, 20)], **{y: [(0, 2), (19, 21)] for y in range(9, 19)}, 19: [(0, 1), (20, 21)], 20: [(1, 1), (20, 20)]}, "H")
        p.spans({16: [(0, 2), (19, 21)], 17: [(0, 2), (19, 21)], 18: [(0, 2), (19, 21)], 19: [(0, 1), (20, 21)], 20: [(1, 1), (20, 20)]}, "P")
    elif m.get("ponytail"):                     # out to the side where the others wear a tail, curling under
        p.spans({7: [(19, 20)], 8: [(19, 21)], 9: [(19, 22)], 10: [(20, 22)], 11: [(20, 22)], 12: [(20, 22)], 13: [(20, 22)],
                 14: [(19, 22)], 15: [(19, 21)], 16: [(18, 20)], 17: [(18, 19)], 18: [(19, 20)]}, "H")
    else:
        return False
    p.shade("H", "h")
    s.outline(PLUM)
    return True


def head(p, m, frame):
    s = p.s
    hair = {5: [(7, 8), (13, 14)], 6: [(3, 16)], 7: [(2, 17)], **{y: [(2, 18)] for y in range(8, 18)}, 18: [(2, 4), (14, 18)], 19: [(2, 3), (17, 18)]}
    p.spans(hair, "H")
    p.pts("H", [(1, 9), (1, 10), (1, 14), (19, 8), (19, 9), (19, 15), (20, 15), (4, 19)])
    if m.get("ahoge"):
        p.pts("H", [(11, 5), (11, 4), (12, 3), (13, 3)])
    p.spans({11: [(6, 6), (8, 9), (11, 12)], **{y: [(5, 13)] for y in range(12, 17)}, 17: [(6, 12)]}, "s")
    p.shade("H", "h")
    hat = m.get("hat")
    if hat == "horned cap":                     # black cap, two little horns, a face on it
        p.spans({3: [(6, 14)], 4: [(4, 16)], 5: [(3, 17)], 6: [(3, 17)], 7: [(2, 18)], 8: [(2, 18)]}, "D")
        p.pts("D", [(5, 1), (5, 2), (6, 2), (15, 1), (15, 2), (14, 2)])
        p.pts("X", [(8, 5), (12, 5), (9, 6), (10, 6), (11, 6)])
    elif hat == "beret":                        # navy beret tipped to our right, a little gold crown
        p.spans({3: [(7, 16)], 4: [(4, 18)], 5: [(3, 19)], 6: [(3, 19)], 7: [(3, 18)]}, "D")
        p.spans({8: [(3, 17)]}, "d")
        p.pts("Z", [(13, 1), (15, 1), (17, 1), (13, 2), (14, 2), (15, 2), (16, 2), (17, 2)])
    s.outline(PLUM)
    if m.get("ponytail"):
        p.pts("D", [(18, 5), (20, 5), (19, 6), (18, 7), (20, 7)])  # its bow
    if hat == "headband":
        p.spans({6: [(4, 16)]}, "Z")
        p.pts("X", [(16, 5), (17, 5), (16, 6), (18, 6), (17, 7), (18, 4)])   # a pink ribbon on our right
    # strands and the inner colours
    p.pts("h", [(9, 8), (6, 9), (14, 9), (5, 10), (9, 10), (13, 10), (3, 13), (3, 14)])
    if m.get("ahoge"):
        p.pts("h", [(18, y) for y in range(11, 18, 2)])
        p.pts("B", [(18, 18)])
    if "R" in m:                                # Mio: the red streak, the yellow clip
        p.pts("R", [(6, 8), (6, 9), (6, 10), (6, 11)])
        p.pts("Z", [(13, 8), (14, 8), (14, 9)])
    if m.get("hat") == "headband":              # AZKi: pink inside the locks
        p.pts("P", [(3, 15), (3, 16), (3, 17), (17, 15), (17, 16), (17, 17)])
    # the face
    eyes = {"blink": [14], "sleep": [14], "happy": []}.get(frame, [12, 13, 14])
    for y in eyes:
        s.px(7, y, PLUM); s.px(11, y, PLUM)
    if frame == "happy":
        p.pts("k", [(6, 13), (7, 12), (8, 13), (10, 13), (11, 12), (12, 13)])
    p.pts("p", [(5, 15), (6, 15), (12, 15), (13, 15)])
    if frame == "happy":
        p.pts("k", [(8, 15), (9, 16), (10, 15)])
    else:
        s.px(9, 15, PLUM)


def draw(member="fubuki", frame="idle"):
    m = MEMBERS[member]
    s = Sprite(W, H)
    p = Pen(s, member)
    breathe = 1 if frame in ("idle2", "walk2", "walk4") else 0
    sleepy = frame == "sleep"
    raised = frame in ("happy", "climb1", "climb2")

    s.layer("tail"); tail(p, m.get("tail"), frame)
    s.layer("body"); body(p, frame)
    s.layer("paws"); paws(p, frame)
    s.layer("ears")
    if ears(p, m.get("ears"), frame) and (breathe or sleepy):
        s.shift(0, 1)
    s.layer("sides")
    if sidetails(p, m) and (breathe or sleepy):
        s.shift(0, 1)
    s.layer("head"); head(p, m, frame)
    if breathe or sleepy:
        s.shift(0, 1)
    if raised:
        s.layer_order.remove("paws")
        s.layer_order.append("paws")
    if sleepy:
        s.layer("fx")
        p.pts("W", [(19, 0), (20, 0), (21, 0), (22, 0), (21, 1), (20, 2), (19, 3), (20, 3), (21, 3), (22, 3)])
    if member == "suisei" and not sleepy:       # her tie's gold star
        s.layer("fx")
        p.pts("Z", [(9, 20 + 0)])
    return s


def framed(member, frame, size=32):
    """The frame on the app's 32 x 32 canvas, feet near the bottom."""
    from PIL import Image
    im = draw(member, frame).composite()
    can = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    can.paste(im, ((size - W) // 2, size - H - 1), im)
    return can


if __name__ == "__main__":
    from PIL import Image
    here = os.path.dirname(os.path.abspath(__file__))
    rows = list(MEMBERS)
    sheet = Image.new("RGBA", (len(FRAMES) * 34, len(rows) * 34), (11, 20, 36, 255))
    for j, member in enumerate(rows):
        for i, f in enumerate(FRAMES):
            c = framed(member, f)
            sheet.paste(c, (i * 34 + 1, j * 34 + 1), c)
    sheet.resize((sheet.width * 4, sheet.height * 4), Image.NEAREST).save(os.path.join(here, "sheet.png"))
    idle = Image.new("RGBA", (len(rows) * 34, 34), (11, 20, 36, 255))
    for j, member in enumerate(rows):
        c = framed(member, "idle")
        idle.paste(c, (j * 34 + 1, 1), c)
    idle.resize((idle.width * 6, idle.height * 6), Image.NEAREST).save(os.path.join(here, "lineup.png"))
