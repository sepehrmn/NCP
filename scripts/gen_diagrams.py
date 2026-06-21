#!/usr/bin/env python3
"""Generate NCP's bespoke "Instrument Datasheet" SVG diagrams (light + dark).

Replaces the flat Mermaid diagrams with hand-composed, GitHub-<img>-safe SVGs:
one semantic design system, depth (gradients + soft shadow + one glow), bespoke
duotone icons, an 8px drafting grid, and one vermillion safety-gated ACTION trace
that dominates each composition. Two committed files per diagram
(``*-light.svg`` + ``*-dark.svg``), embedded via a ``prefers-color-scheme``
``<picture>`` (mirrors docs/plots/). Palette reuses the perf-plot hues verbatim
so diagrams and benchmarks read as one instrument.

Output: docs/diagrams/{topology,ecosystem,versioning,fsm,sequence}-{light,dark}.svg
Run:    python3 scripts/gen_diagrams.py    (from repo root)

Pure stdlib. GitHub-safe: gradients/filters/patterns/markers/real <text> only —
no <script>, <foreignObject>, external href/font/CSS, animation, or interactivity.
"""
from __future__ import annotations
import os

# ───────────────────────────── theme tokens ─────────────────────────────
DARK = dict(
    name="dark",
    bg_top="#11161d", bg_bot="#0d1117",
    surf_top="#161b22", surf_bot="#11161d", surf_chip="#1b232c",
    border="#30363d",
    grid="#8b949e", grid_dot_op=0.5, grid_major_op=0.22,
    tprim="#e6edf3", tsec="#c9d1d9", tmut="#8b949e",
    control="#3a9ad9", perception="#56b4e9", action="#e8783c", action_hi="#ff8a4c",
    observation="#8b949e", contract="#a78bfa", contract_lo="#7c3aed",
    active="#33c295", hold="#f0b429", configfail="#e08cbf",
    shadow="#05070b", shadow_op=0.6, shadow_dy=4, shadow_sd=7,
    halo_op=0.40, glow_flood="#ff8a4c", glow_op=0.9, glow_double=True,
    bus_stops=[("0", "#e8783c", "0.85"), ("0.5", "#ff8a4c", "1"), ("1", "#e8783c", "0.85")],
    wash_op=0.06,
)
LIGHT = dict(
    name="light",
    bg_top="#ffffff", bg_bot="#f3f5f8",
    surf_top="#ffffff", surf_bot="#eef1f5", surf_chip="#eef1f5",
    border="#d0d7de",
    grid="#57606a", grid_dot_op=0.5, grid_major_op=0.26,
    tprim="#1b2733", tsec="#24292f", tmut="#57606a",
    control="#0072B2", perception="#56B4E9", action="#D55E00", action_hi="#D55E00",
    observation="#999999", contract="#6D28D9", contract_lo="#5b21b6",
    active="#009E73", hold="#E69F00", configfail="#CC79A7",
    shadow="#1b2733", shadow_op=0.18, shadow_dy=3, shadow_sd=5,
    halo_op=0.22, glow_flood="#D55E00", glow_op=0.4, glow_double=False,
    bus_stops=[("0", "#D55E00", "1"), ("0.5", "#D55E00", "1"), ("1", "#b94f00", "1")],
    wash_op=0.05,
)

SANS = "-apple-system, BlinkMacSystemFont, 'Segoe UI', 'Noto Sans', Helvetica, Arial, sans-serif"
MONO = "ui-monospace, SFMono-Regular, 'SF Mono', Menlo, Consolas, 'Liberation Mono', monospace"


# ───────────────────────────── primitives ─────────────────────────────
def esc(s: str) -> str:
    return (s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;"))


def T(x, y, s, size, weight=400, fill="#000", *, family=SANS, anchor="start",
      track=0, italic=False, op=None, mono=False):
    fam = MONO if mono else family
    style = "italic" if italic else "normal"
    extra = f' letter-spacing="{track}"' if track else ""
    opa = f' fill-opacity="{op}"' if op is not None else ""
    return (f'<text x="{x}" y="{y}" font-family="{fam}" font-size="{size}" '
            f'font-weight="{weight}" font-style="{style}" fill="{fill}"{opa} '
            f'text-anchor="{anchor}"{extra} text-rendering="geometricPrecision">{esc(s)}</text>')


def rect(x, y, w, h, rx=0, fill="none", stroke="none", sw=0, dash=None, op=None, filt=None):
    d = f' stroke-dasharray="{dash}"' if dash else ""
    o = f' fill-opacity="{op}"' if op is not None else ""
    f = f' filter="url(#{filt})"' if filt else ""
    s = f' stroke="{stroke}" stroke-width="{sw}"' if stroke != "none" else ""
    return f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{rx}" fill="{fill}"{s}{d}{o}{f}/>'


def line(x1, y1, x2, y2, stroke, sw, dash=None, cap="round", op=None, marker=None, filt=None):
    d = f' stroke-dasharray="{dash}"' if dash else ""
    o = f' stroke-opacity="{op}"' if op is not None else ""
    m = f' marker-end="url(#{marker})"' if marker else ""
    f = f' filter="url(#{filt})"' if filt else ""
    return (f'<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" stroke="{stroke}" '
            f'stroke-width="{sw}" stroke-linecap="{cap}"{d}{o}{m}{f}/>')


def path(d, stroke="none", sw=0, fill="none", cap="round", join="round", op=None, marker=None, filt=None, dash=None):
    o = f' stroke-opacity="{op}"' if op is not None else ""
    m = f' marker-end="url(#{marker})"' if marker else ""
    f = f' filter="url(#{filt})"' if filt else ""
    dd = f' stroke-dasharray="{dash}"' if dash else ""
    s = f' stroke="{stroke}" stroke-width="{sw}" stroke-linecap="{cap}" stroke-linejoin="{join}"' if stroke != "none" else ""
    return f'<path d="{d}" fill="{fill}"{s}{dd}{o}{m}{f}/>'


# ───────────────────────────── defs kit ─────────────────────────────
def defs(th) -> str:
    bus = "".join(f'<stop offset="{o}" stop-color="{c}" stop-opacity="{op}"/>' for o, c, op in th["bus_stops"])
    glow_merge = ('<feMergeNode in="g"/><feMergeNode in="g"/><feMergeNode in="SourceGraphic"/>'
                  if th["glow_double"] else '<feMergeNode in="g"/><feMergeNode in="SourceGraphic"/>')
    glow_in = "SourceGraphic" if th["glow_double"] else "SourceAlpha"
    return f'''<defs>
  <linearGradient id="pageBg" x1="0" y1="0" x2="0" y2="1">
    <stop offset="0" stop-color="{th['bg_top']}"/><stop offset="1" stop-color="{th['bg_bot']}"/>
  </linearGradient>
  <linearGradient id="surface" x1="0" y1="0" x2="0" y2="1">
    <stop offset="0" stop-color="{th['surf_top']}"/><stop offset="1" stop-color="{th['surf_bot']}"/>
  </linearGradient>
  <linearGradient id="busAction" x1="0" y1="0" x2="1" y2="0">{bus}</linearGradient>
  <linearGradient id="contractHero" x1="0" y1="0" x2="1" y2="1">
    <stop offset="0" stop-color="{th['contract']}"/><stop offset="1" stop-color="{th['contract_lo']}"/>
  </linearGradient>
  <radialGradient id="gridDot" cx="0.5" cy="0.5" r="0.5">
    <stop offset="0" stop-color="{th['grid']}" stop-opacity="{th['grid_dot_op']}"/>
    <stop offset="1" stop-color="{th['grid']}" stop-opacity="0"/>
  </radialGradient>
  <pattern id="grid" width="22" height="22" patternUnits="userSpaceOnUse">
    <circle cx="2" cy="2" r="1.4" fill="url(#gridDot)"/>
  </pattern>
  <pattern id="gridMajor" width="110" height="110" patternUnits="userSpaceOnUse">
    <path d="M110 0H0V110" fill="none" stroke="{th['grid']}" stroke-width="0.75" stroke-opacity="{th['grid_major_op']}"/>
  </pattern>
  <filter id="soft" x="-40%" y="-40%" width="180%" height="180%" color-interpolation-filters="sRGB">
    <feDropShadow dx="0" dy="{th['shadow_dy']}" stdDeviation="{th['shadow_sd']}" flood-color="{th['shadow']}" flood-opacity="{th['shadow_op']}"/>
  </filter>
  <filter id="halo" x="-60%" y="-60%" width="220%" height="220%" color-interpolation-filters="sRGB">
    <feGaussianBlur stdDeviation="3"/>
  </filter>
  <filter id="glow" x="-90%" y="-90%" width="280%" height="280%" color-interpolation-filters="sRGB">
    <feGaussianBlur in="{glow_in}" stdDeviation="3.4" result="b"/>
    <feFlood flood-color="{th['glow_flood']}" flood-opacity="{th['glow_op']}"/>
    <feComposite in2="b" operator="in" result="g"/>
    <feMerge>{glow_merge}</feMerge>
  </filter>
  <marker id="arrowAction" markerWidth="9" markerHeight="9" refX="5" refY="4" orient="auto"><path d="M0,0 L7,4 L0,8 Z" fill="{th['action']}"/></marker>
  <marker id="arrowControl" markerWidth="9" markerHeight="9" refX="5" refY="4" orient="auto"><path d="M0,0 L7,4 L0,8 Z" fill="{th['control']}"/></marker>
  <marker id="replyControl" markerWidth="9" markerHeight="9" refX="3" refY="4" orient="auto"><path d="M7,0 L0,4 L7,8" fill="none" stroke="{th['control']}" stroke-width="1.4" stroke-linejoin="round"/></marker>
  <marker id="arrowPercep" markerWidth="9" markerHeight="9" refX="5" refY="4" orient="auto"><path d="M0,0 L7,4 L0,8 Z" fill="{th['perception']}"/></marker>
  <marker id="tapObserve" markerWidth="10" markerHeight="10" refX="4.5" refY="4.5" orient="auto"><circle cx="4.5" cy="4.5" r="3" fill="none" stroke="{th['observation']}" stroke-width="1.3"/></marker>
  <marker id="arrowContract" markerWidth="9" markerHeight="9" refX="5" refY="4" orient="auto"><path d="M0,0 L7,4 L0,8 Z" fill="{th['contract']}"/></marker>
  <marker id="submoduleArrow" markerWidth="10" markerHeight="10" refX="6" refY="4.5" orient="auto"><path d="M2,1 L7,4.5 L2,8" fill="none" stroke="{th['observation']}" stroke-width="1.4" stroke-linejoin="round"/></marker>
  <marker id="arrowActive" markerWidth="9" markerHeight="9" refX="5" refY="4" orient="auto"><path d="M0,0 L7,4 L0,8 Z" fill="{th['active']}"/></marker>
  <marker id="arrowEstop" markerWidth="9" markerHeight="9" refX="5" refY="4" orient="auto"><path d="M0,0 L7,4 L0,8 Z" fill="{th['action']}"/></marker>
  <marker id="arrowHold" markerWidth="9" markerHeight="9" refX="5" refY="4" orient="auto"><path d="M0,0 L7,4 L0,8 Z" fill="{th['hold']}"/></marker>
  <marker id="arrowMut" markerWidth="9" markerHeight="9" refX="5" refY="4" orient="auto"><path d="M0,0 L7,4 L0,8 Z" fill="{th['tmut']}"/></marker>
  <filter id="glowContract" x="-90%" y="-90%" width="280%" height="280%" color-interpolation-filters="sRGB">
    <feGaussianBlur in="{glow_in}" stdDeviation="3.4" result="b"/>
    <feFlood flood-color="{th['contract']}" flood-opacity="{th['glow_op']}"/>
    <feComposite in2="b" operator="in" result="g"/>
    <feMerge>{glow_merge}</feMerge>
  </filter>
  <filter id="glowActive" x="-90%" y="-90%" width="280%" height="280%" color-interpolation-filters="sRGB">
    <feGaussianBlur in="{glow_in}" stdDeviation="3.4" result="b"/>
    <feFlood flood-color="{th['active']}" flood-opacity="{th['glow_op']}"/>
    <feComposite in2="b" operator="in" result="g"/>
    <feMerge>{glow_merge}</feMerge>
  </filter>
</defs>'''


# ───────────────────────────── bespoke icons (24x24 → placed) ─────────────────────────────
def _icon(inner, x, y, size, hue):
    s = size / 24.0
    return (f'<g transform="translate({x},{y}) scale({s:.4f})" fill="none" stroke="{hue}" '
            f'stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{inner}</g>')


def ic_brain(x, y, size, hue):
    inner = (f'<path d="M9.5 5.2A3.2 3.2 0 0 0 4 7.6a3 3 0 0 0-1 5.6a3.2 3.2 0 0 0 4 3.6a2.6 2.6 0 0 0 2.5 1.6"/>'
             f'<path d="M14.5 5.2A3.2 3.2 0 0 1 20 7.6a3 3 0 0 1 1 5.6a3.2 3.2 0 0 1-4 3.6a2.6 2.6 0 0 1-2.5 1.6"/>'
             f'<path d="M12 5v13.4"/>'
             f'<circle cx="12" cy="5" r="0.5" fill="{hue}" stroke="{hue}"/>'
             f'<circle cx="7.5" cy="9" r="0.5" fill="{hue}" stroke="{hue}"/>'
             f'<circle cx="16.5" cy="9" r="0.5" fill="{hue}" stroke="{hue}"/>'
             f'<circle cx="8" cy="14" r="0.5" fill="{hue}" stroke="{hue}"/>'
             f'<circle cx="16" cy="14" r="0.5" fill="{hue}" stroke="{hue}"/>'
             f'<path d="M12 8.2 7.5 9M12 8.2 16.5 9M12 12.5 8 14M12 12.5 16 14"/>')
    return _icon(inner, x, y, size, hue)


def ic_robot(x, y, size, hue):
    inner = (f'<rect x="5" y="8" width="14" height="11" rx="3"/>'
             f'<path d="M12 8V5"/>'
             f'<circle cx="12" cy="3.6" r="1.4" fill="{hue}" stroke="none"/>'
             f'<circle cx="9" cy="12.5" r="1.2" fill="{hue}" stroke="none"/>'
             f'<circle cx="15" cy="12.5" r="1.2" fill="{hue}" stroke="none"/>'
             f'<path d="M9.5 16h5"/><path d="M5 12.5H3M19 12.5h2"/>')
    return _icon(inner, x, y, size, hue)


def ic_eye(x, y, size, hue):
    inner = (f'<path d="M2.5 12c2.2-4 6-6 9.5-6s7.3 2 9.5 6c-2.2 4-6 6-9.5 6s-7.3-2-9.5-6Z"/>'
             f'<circle cx="12" cy="12" r="3"/>')
    return _icon(inner, x, y, size, hue)


def ic_key(x, y, size, hue):
    inner = (f'<circle cx="7.5" cy="7.5" r="3.5"/><path d="M10 10l8.5 8.5"/>'
             f'<path d="M16 16l2-2M18.5 18.5l2-2"/>')
    return _icon(inner, x, y, size, hue)


def ic_lock(x, y, size, hue):
    inner = f'<rect x="5" y="11" width="14" height="9" rx="2.5"/><path d="M8 11V8a4 4 0 0 1 8 0v3"/><path d="M12 14.5v2.5"/>'
    return _icon(inner, x, y, size, hue)


def ic_power(x, y, size, hue):
    inner = f'<path d="M12 3v8"/><path d="M7.6 6.2a8 8 0 1 0 8.8 0"/>'
    return _icon(inner, x, y, size, hue)


def ic_pause(x, y, size, hue):
    inner = f'<path d="M9 6v12M15 6v12"/>'
    return _icon(inner, x, y, size, hue)


def ic_warn(x, y, size, hue):
    inner = (f'<path d="M12 4.3 20.6 19.2a1 1 0 0 1-0.87 1.5H4.27a1 1 0 0 1-0.87-1.5L12 4.3Z"/>'
             f'<path d="M12 10v3.6"/><circle cx="12" cy="16.8" r="1.05" fill="{hue}" stroke="none"/>')
    return _icon(inner, x, y, size, hue)


def ic_antenna(x, y, size, hue):
    inner = (f'<path d="M12 13v7"/><circle cx="12" cy="11" r="1.6" fill="{hue}" stroke="none"/>'
             f'<path d="M8.8 14.2a4.5 4.5 0 0 1 0-6.4M15.2 7.8a4.5 4.5 0 0 1 0 6.4"/>'
             f'<path d="M6.5 16.5a7.8 7.8 0 0 1 0-11M17.5 5.5a7.8 7.8 0 0 1 0 11"/><path d="M9.5 20h5"/>')
    return _icon(inner, x, y, size, hue)


def ic_book(x, y, size, hue):
    inner = (f'<path d="M5 5.5A1.5 1.5 0 0 1 6.5 4H19v15H6.5A1.5 1.5 0 0 0 5 20.5Z"/>'
             f'<path d="M5 5.5v15"/><path d="M9 8h6M9 11h6"/>')
    return _icon(inner, x, y, size, hue)


def ic_gauge(x, y, size, hue):
    inner = (f'<path d="M3.5 17.5a8.5 8.5 0 0 1 17 0"/><path d="M12 17.5 16.2 11.8"/>'
             f'<circle cx="12" cy="17.5" r="1.5" fill="{hue}" stroke="none"/>')
    return _icon(inner, x, y, size, hue)


def ic_break(x, y, size, hue):
    inner = f'<path d="M13 3 L8.5 11 H12.5 L10 21 L17 10 H12.5 L15 3 Z" fill="{hue}" fill-opacity="0.18"/>'
    return _icon(inner, x, y, size, hue)


def ic_noentry(x, y, size, hue):
    inner = f'<circle cx="12" cy="12" r="8.5"/><path d="M6.5 12 H17.5"/>'
    return _icon(inner, x, y, size, hue)


def ic_play(x, y, size, hue):
    inner = f'<circle cx="12" cy="12" r="8.5"/><path d="M10 8 L16 12 L10 16 Z" fill="{hue}" stroke="none"/>'
    return _icon(inner, x, y, size, hue)


def ic_approx(x, y, size, hue):
    inner = '<path d="M3 9.5q3 -3.5 6 0t6 0"/><path d="M3 15q3 -3.5 6 0t6 0"/>'
    return _icon(inner, x, y, size, hue)


def ic_octagon(x, y, size, hue):
    inner = f'<path d="M8.5 3.5 H15.5 L20.5 8.5 V15.5 L15.5 20.5 H8.5 L3.5 15.5 V8.5 Z"/><path d="M7.5 12 H16.5"/>'
    return _icon(inner, x, y, size, hue)


def ic_terminal(x, y, size, hue):
    inner = f'<rect x="3" y="5" width="18" height="14" rx="2.5"/><path d="M7 10l3 2.5-3 2.5"/><path d="M12.5 15h4.5"/>'
    return _icon(inner, x, y, size, hue)


def ic_wave(x, y, size, hue):
    inner = (f'<rect x="3" y="5" width="18" height="14" rx="2.5"/>'
             f'<path d="M6 12h2l1.5-3 2 6 1.5-4 1 1h3"/>')
    return _icon(inner, x, y, size, hue)


def diamond(cx, cy, d, th, ring_hue):
    """Rounded decision diamond (rotated rounded square) with soft shadow + identity ring."""
    L = d * 1.414
    x, y = cx - L / 2, cy - L / 2
    tr = f'rotate(45 {cx} {cy})'
    return (f'<g transform="{tr}">'
            + rect(x, y, L, L, rx=16, fill="url(#surface)", stroke="none", filt="soft")
            + rect(x, y, L, L, rx=16, fill="none", stroke=th["border"], sw=1.5)
            + rect(x + 5, y + 5, L - 10, L - 10, rx=12, fill="none", stroke=ring_hue, sw=2, op=0.85)
            + '</g>')


# ───────────────────────────── components ─────────────────────────────
def background(th, w, h):
    return (rect(0, 0, w, h, fill="url(#pageBg)")
            + rect(0, 0, w, h, fill="url(#gridMajor)")
            + rect(0, 0, w, h, fill="url(#grid)"))


def reg_ticks(th, x, y, w, h):
    """Two 6px L-bracket registration ticks at opposite (TR + BL) corners."""
    c = th["tmut"]
    tr = path(f"M{x+w-6},{y} L{x+w},{y} L{x+w},{y+6}", stroke=c, sw=1, op=0.5)
    bl = path(f"M{x},{y+h-6} L{x},{y+h} L{x+6},{y+h}", stroke=c, sw=1, op=0.5)
    return tr + bl


def card(th, x, y, w, h, rail_hue, designator, *, dashed=False, glow=False, filled=None):
    """Surface card: soft shadow, optional dashed enclosure, left accent rail, designator well."""
    out = []
    if filled:
        out.append(rect(x, y, w, h, rx=12, fill=filled, stroke=th["border"], sw=1.5,
                        filt="glow" if glow else "soft"))
    else:
        stroke = rail_hue if dashed else th["border"]
        dash = "5 4" if dashed else None
        op = 0.7 if dashed else None
        # shadow + surface
        out.append(rect(x, y, w, h, rx=12, fill="url(#surface)", stroke="none", filt="soft"))
        out.append(rect(x, y, w, h, rx=12, fill="none", stroke=stroke, sw=1.5, dash=dash, op=op))
    # accent rail (skip for filled hero where the fill IS the identity)
    if not filled:
        out.append(rect(x + 6, y + 12, 4, h - 24, rx=2, fill=rail_hue))
    out.append(reg_ticks(th, x, y, w, h))
    # designator well
    dw, dh = 26, 15
    out.append(rect(x + 14, y + 10, dw, dh, rx=4, fill=th["surf_chip"], stroke=th["border"], sw=1))
    out.append(T(x + 27, y + 20.5, designator, 10, 700, th["tsec"], mono=True, anchor="middle"))
    return "".join(out)


def svg_open(w, h):
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" '
            f'viewBox="0 0 {w} {h}" font-family="{SANS}" role="img">')


def sheet_meta(th, x, y, s):
    return T(x, y, s, 10, 600, th["tmut"], mono=True, anchor="end", track=0.8)


def title_block(th, title, eyebrow, w):
    out = [T(28, 50, title, 24, 800, th["tprim"], track=-0.3)]
    out.append(T(29, 64, eyebrow, 10.5, 600, th["tmut"], track=1.0))
    out.append(line(28, 74, w - 28, 74, th["border"], 1, cap="butt"))
    return "".join(out)


# ───────────────────────────── 1. TOPOLOGY (hero) ─────────────────────────────
def topology(th):
    W, H = 860, 610
    s = [svg_open(W, H), defs(th), background(th, W, H)]
    s.append(title_block(th, "TOPOLOGY",
                         "COMMANDER ⇄ BODY  ·  4 QoS PLANES  ·  OBSERVER ATTACHES FOR FREE", W))
    s.append(sheet_meta(th, W - 28, 48, "NCP · WIRE 0.5 · CONTRACT 24e8e6e31e1dec8a"))

    # node coords
    U1 = (44, 252, 200, 96)    # commander  (center 144,300)
    U2 = (616, 252, 200, 96)   # body       (center 716,300)
    O1 = (298, 466, 264, 76)   # observer (lower bay)

    # ---- edges (painted bottom-up: OBSERVATION, PERCEPTION, CONTROL, then ACTION on top) ----
    # OBSERVATION O1a/O1b — both cards drop dotted to observer (junction y=442)
    obs = th["observation"]
    s.append(path("M144,348 L144,438 Q144,442 148,442 L356,442 Q360,442 360,446 L360,466",
                  stroke=obs, sw=1.5, dash="3 3", marker="tapObserve"))
    s.append(path("M716,348 L716,438 Q716,442 712,442 L504,442 Q500,442 500,446 L500,466",
                  stroke=obs, sw=1.5, dash="3 3", marker="tapObserve"))
    # PERCEPTION P1 (body → commander), dashed, lane y=210 (risers offset from CONTROL)
    per = th["perception"]
    s.append(path("M680,252 L680,210 L184,210 L184,252", stroke=per, sw=2.5, dash="6 4", marker="arrowPercep"))
    # CONTROL C1 (commander ⇄ body), solid, lane y=168, bidirectional
    ctl = th["control"]
    s.append(path("M120,252 L120,168 L740,168 L740,252", stroke=ctl, sw=2.5, marker="arrowControl"))
    s.append(path("M126,174 L120,168", stroke=ctl, sw=2.5, marker="replyControl"))  # reply chevron at commander
    # ACTION A1 — the hero bus, dead-straight at y=300
    act = th["action"]
    s.append(line(244, 300, 616, 300, act, 9, op=th["halo_op"], filt="halo"))
    s.append(line(244, 300, 616, 300, "url(#busAction)", 4, marker="arrowAction"))
    for tx in (300, 372, 444, 516):
        s.append(line(tx, 295, tx, 305, act, 1, op=0.7))

    # ---- cards ----
    s.append(card(th, *U1, th["control"], "U1"))
    s.append(ic_brain(58, 286, 24, th["control"]))
    s.append(T(90, 300, "NEST brain", 14, 700, th["tprim"]))
    s.append(T(90, 316, "the commander", 10.5, 500, th["tsec"]))
    s.append(T(90, 329, "point + rate neurons", 10.5, 500, th["tsec"]))

    s.append(card(th, *U2, th["observation"], "U2"))
    s.append(ic_robot(630, 286, 24, th["observation"]))
    s.append(T(662, 300, "robot / UAV body", 14, 700, th["tprim"]))
    s.append(T(662, 316, "the plant", 10.5, 500, th["tsec"]))
    s.append(T(662, 329, "example: crebain", 10.5, 500, th["tsec"]))

    s.append(card(th, *O1, th["observation"], "O1", dashed=True))
    s.append(ic_eye(312, 494, 22, th["observation"]))
    s.append(T(342, 500, "analysis / observer client", 14, 700, th["tprim"]))
    s.append(T(342, 515, "attaches for free · read-only tap", 10.5, 500, th["tsec"]))

    # ---- plane label chips (knockout pill on the edge) ----
    def chip(cx, cy, w, desig, dh_hue, concept, key, body, h=30):
        x = cx - w / 2
        y = cy - h / 2
        o = [rect(x - 1.5, y - 1.5, w + 3, h + 3, rx=9, fill=th["bg_bot"]),   # knockout
             rect(x, y, w, h, rx=8, fill=th["surf_chip"], stroke=th["border"], sw=1),
             rect(x + 6, y + 7, 16, 16, rx=3, fill=dh_hue),
             T(x + 14, y + 18.5, desig, 9, 700, "#fff" if th["name"] == "dark" else "#fff", mono=True, anchor="middle"),
             T(x + 28, y + 13, concept, 10.5, 700, dh_hue, track=1.0),
             T(x + 28, y + 25, key, 9.5, 500, th["tmut"], mono=True)]
        if body:
            o.append(T(x + 28 + len(concept) * 7 + 14, y + 13, body, 9.5, 500, th["tsec"]))
        return "".join(o)

    s.append(chip(430, 168, 268, "C1", ctl, "CONTROL", "{realm}/rpc", "reliable · request/reply · queryable"))
    s.append(chip(430, 210, 268, "P1", per, "PERCEPTION", "{realm}/session/{id}/sensor[/{name}]", "best-effort · DROP · lossy-OK"))
    s.append(chip(430, 442, 250, "O1", obs, "OBSERVATION", "{realm}/session/{id}/observation", "read-only tap"))

    # ---- THE HERO: ACTION chip (the one glow) ----
    aw, ah = 336, 96
    ax = int(430 - aw / 2)     # 262
    ay = 326
    s.append(line(430, 300, 430, ay, act, 2))  # connector bus → chip
    s.append(rect(ax, ay, aw, ah, rx=10, fill="url(#surface)", stroke=act, sw=1.5, filt="glow"))
    s.append(rect(ax, ay, aw, ah, rx=10, fill=act, op=th["wash_op"]))
    # header row: designator · ACTION eyebrow · right tags
    s.append(rect(ax + 14, ay + 13, 20, 20, rx=4, fill=act))
    s.append(T(ax + 24, ay + 27, "A1", 10, 700, "#ffffff", mono=True, anchor="middle"))
    s.append(T(ax + 42, ay + 27, "ACTION", 12, 700, act, track=1.6))
    s.append(T(ax + aw - 16, ay + 27, "express · RealTime · safety-gated", 9, 600, act, anchor="end", op=0.95))
    # wire key row
    s.append(T(ax + 14, ay + 46, "{realm}/session/{id}/command[/{name}]", 9.5, 500, th["tmut"], mono=True))
    # mode-pill row — the wire enum made visible {init·active·hold·estop}
    py = ay + 56
    s.append(T(ax + 14, py + 13, "mode", 9, 600, th["tmut"], mono=True))
    pills = [("init", None, th["surf_chip"], th["tsec"]),
             ("active", True, th["active"], "#06281e" if th["name"] == "dark" else "#ffffff"),
             ("hold", False, th["hold"], th["hold"]),
             ("estop", True, th["action"], "#ffffff")]
    px = ax + 50
    for label, filled, col, txt in pills:
        pw = 10 + len(label) * 6.4
        if filled is True:
            s.append(rect(px, py, pw, 19, rx=6, fill=col))
        elif filled is None:
            s.append(rect(px, py, pw, 19, rx=6, fill=col, stroke=th["border"], sw=1))
        else:
            s.append(rect(px, py, pw, 19, rx=6, fill="none", stroke=col, sw=1.3))
        s.append(T(px + pw / 2, py + 13, label, 9, 700, txt, anchor="middle", mono=True))
        px += pw + 7
    s.append(T(px + 4, py + 13, "· ttl_ms", 9, 600, th["hold"], mono=True))
    # footnote
    s.append(T(ax + 14, ay + ah - 10, "mode = explicit wire authority · HOLD/ESTOP fail-safe to zero",
               8.5, 500, th["tmut"], italic=True, track=0.2))

    # ---- bottom legend rail ----
    ly = 568
    s.append(rect(28, ly, W - 56, 28, rx=8, fill=th["surf_chip"], stroke=th["border"], sw=1))
    legend = [("A1", act, "ACTION", "heaviest · safety-gated", 4, None),
              ("C1", ctl, "CONTROL", "reliable · queryable", 2.5, None),
              ("P1", per, "PERCEPTION", "best-effort", 2.5, "6 4"),
              ("O1", obs, "OBSERVATION", "read-only tap", 1.5, "3 3")]
    lx = 48
    for desig, hue, concept, tail, sw, dash in legend:
        s.append(T(lx, ly + 18, desig, 10, 700, hue, mono=True))
        s.append(line(lx + 22, ly + 14, lx + 44, ly + 14, hue, sw, dash=dash))
        s.append(T(lx + 52, ly + 18, concept, 10.5, 700, hue, track=0.6))
        s.append(T(lx + 52 + len(concept) * 7.4 + 8, ly + 18, tail, 9.5, 500, th["tmut"]))
        lx += 200
    s.append("</svg>")
    return "".join(s)


TOPOLOGY_ALT = ("NCP topology: one Commander (a NEST-brain neuromorphic controller, U1) coordinates one "
    "Body/plant (robot or UAV, U2) over four QoS planes, plus a read-only Observer client (O1) that "
    "attaches for free. The safety-gated ACTION plane is the focal element — the heaviest, brightest "
    "vermillion trace running dead-center from Commander to Body, carrying {realm}/session/{id}/command"
    "[/{name}] as express, RealTime, safety-gated traffic with a visible mode enum (init, active, hold, "
    "estop — estop flagged danger-red) and a ttl_ms HOLD fail-safe. CONTROL ({realm}/rpc) is a reliable, "
    "bidirectional request/reply rail (queryable). PERCEPTION ({realm}/session/{id}/sensor[/{name}]) is a "
    "dashed best-effort DROP plane from Body to Commander. OBSERVATION ({realm}/session/{id}/observation) "
    "is a dotted read-only tap published by both Commander and Body to the Observer. NCP wire 0.5, "
    "contract hash 24e8e6e31e1dec8a.")


# ───────────────────────────── 2. ECOSYSTEM ─────────────────────────────
def ecosystem(th):
    W, H = 820, 520
    s = [svg_open(W, H), defs(th), background(th, W, H)]
    s.append(title_block(th, "ECOSYSTEM",
                         "NCP IS THE WIRE CONTRACT ONLY  ·  EXAMPLE PEERS EACH RE-PIN TAG v0.5.0", W))
    s.append(sheet_meta(th, W - 28, 48, "NCP · WIRE 0.5 · CONTRACT 24e8e6e31e1dec8a"))
    ctr, obs, ctl = th["contract"], th["observation"], th["control"]

    # pin + submodule edges (under the cards)
    for d in ("M300,192 L342,192 Q350,192 350,200 L350,242 Q350,250 358,250 L380,250",
              "M300,298 L380,298",
              "M300,404 L342,404 Q350,404 350,396 L350,328 Q350,320 358,320 L380,320"):
        s.append(path(d, stroke=ctr, sw=2, marker="arrowContract"))
    for tx, ty in ((318, 298), (348, 298)):
        s.append(line(tx, 294, tx, 302, ctr, 1, op=0.7))
    s.append(path("M300,430 L632,430", stroke=obs, sw=1.5, dash="6 4", marker="submoduleArrow"))

    # consumer cards (left column)
    for x, y, des, hue, icon, name, sub in [
            (56, 150, "E1", ctl, ic_brain, "Engram / Paper2Brain", "example commander · NEST + SessionService"),
            (56, 256, "C1", obs, ic_robot, "crebain", "example body · robot / UAV plant"),
            (56, 362, "P1", obs, ic_eye, "pid_vla", "example analysis · observer client")]:
        s.append(card(th, x, y, 244, 84, hue, des))
        s.append(icon(x + 18, y + 40, 24, hue))
        s.append(T(x + 52, y + 48, name, 14, 700, th["tprim"]))
        s.append(T(x + 52, y + 64, sub, 10, 500, th["tsec"]))

    # HERO contract hub (filled + glow)
    hx, hy, hw, hh = 380, 196, 240, 164
    cx = hx + hw / 2
    s.append(rect(hx, hy, hw, hh, rx=12, fill="url(#contractHero)", stroke=th["border"], sw=1.5, filt="glowContract"))
    s.append(rect(hx + 14, hy + 13, hw - 28, 2, rx=1, fill="#ffffff", op=0.5))
    s.append(rect(hx + 16, hy + 14, 26, 15, rx=4, fill="#ffffff", op=0.16))
    s.append(T(hx + 29, hy + 24.5, "U1", 10, 700, "#ffffff", mono=True, anchor="middle"))
    s.append(ic_key(cx - 14, hy + 30, 28, "#ffffff"))
    s.append(T(cx, hy + 82, "NCP", 17, 800, "#ffffff", anchor="middle"))
    s.append(T(cx, hy + 99, "the wire contract", 11, 600, "#ffffff", anchor="middle", op=0.92))
    s.append(T(cx, hy + 120, "ncp-core · ncp-zenoh · ncp-gateway", 9.5, 600, "#ffffff", anchor="middle", op=0.9, mono=True))
    s.append(line(hx + 22, hy + 128, hx + hw - 22, hy + 128, "#ffffff", 1, op=0.18))
    s.append(T(cx, hy + 140, "peers: ncp-python · ncp-cpp · @sepehrmn/ncp", 8.5, 500, "#ffffff", anchor="middle", op=0.76, mono=True))
    s.append(rect(cx - 62, hy + 147, 124, 15, rx=6, fill="#ffffff", op=0.13))
    s.append(T(cx, hy + 157.5, "WIRE 0.5 · 24e8e6e3", 9, 700, "#ffffff", anchor="middle", mono=True))

    # pid-rs pendant (quarantined: dashed, no rail, muted)
    qx, qy, qw, qh = 632, 388, 152, 84
    s.append(rect(qx, qy, qw, qh, rx=12, fill="url(#surface)", stroke="none", filt="soft"))
    s.append(rect(qx, qy, qw, qh, rx=12, fill="none", stroke=th["border"], sw=1.5, dash="6 4"))
    s.append(reg_ticks(th, qx, qy, qw, qh))
    s.append(rect(qx + 14, qy + 10, 26, 15, rx=4, fill=th["surf_chip"], stroke=th["border"], sw=1))
    s.append(T(qx + 27, qy + 20.5, "L1", 10, 700, th["tmut"], mono=True, anchor="middle"))
    s.append(ic_book(qx + 16, qy + 42, 20, th["tmut"]))
    s.append(T(qx + 44, qy + 48, "pid-rs", 13, 700, th["tmut"]))
    s.append(T(qx + 44, qy + 63, "PID estimators · science lib", 9, 500, th["tmut"]))

    # pin chips (the three identical v0.5.0 LEDs) + submodule chip
    def pinchip(cx_, cy_):
        w, h = 64, 22
        x, y = cx_ - w / 2, cy_ - h / 2
        return (rect(x - 1.5, y - 1.5, w + 3, h + 3, rx=8, fill=th["bg_bot"])
                + rect(x, y, w, h, rx=8, fill=th["surf_chip"], stroke=th["border"], sw=1)
                + rect(x + 7, y + 6, 10, 10, rx=2, fill=ctr)
                + T(x + 23, y + 15, "v0.5.0", 9.5, 700, th["tsec"], mono=True))
    s.append(pinchip(326, 192))
    s.append(pinchip(335, 298))
    s.append(pinchip(326, 404))
    sw_, sh_ = 214, 30
    sx, sy = 466 - sw_ / 2, 410 - sh_ / 2
    s.append(rect(sx - 1.5, sy - 1.5, sw_ + 3, sh_ + 3, rx=8, fill=th["bg_bot"]))
    s.append(rect(sx, sy, sw_, sh_, rx=8, fill=th["surf_chip"], stroke=obs, sw=1, dash="4 4", op=0.85))
    s.append(T(sx + 12, sy + 13, "git submodule", 10.5, 700, th["tmut"]))
    s.append(T(sx + 12, sy + 24, "NOT an NCP wire consumer", 9.5, 500, th["tmut"], italic=True))

    # legend rail
    ly = 488
    s.append(rect(28, ly, W - 56, 26, rx=8, fill=th["surf_chip"], stroke=th["border"], sw=1))
    s.append(line(48, ly + 13, 70, ly + 13, ctr, 2))
    s.append(T(78, ly + 17, "pin tag v0.5.0 (depends-on)", 9.5, 600, th["tsec"]))
    s.append(line(300, ly + 13, 322, ly + 13, obs, 1.5, dash="6 4"))
    s.append(T(330, ly + 17, "git submodule · NOT an NCP wire consumer", 9.5, 600, th["tsec"]))
    s.append(rect(640, ly + 8, 12, 10, rx=2, fill="url(#contractHero)"))
    s.append(T(658, ly + 17, "the wire contract (only filled node)", 9.5, 600, th["tsec"]))
    s.append("</svg>")
    return "".join(s)


ECOSYSTEM_ALT = ("NCP ecosystem: a single highlighted NCP wire-contract node at center (crates ncp-core, "
    "ncp-zenoh, ncp-gateway; peers ncp-python, ncp-cpp, @sepehrmn/ncp; wire 0.5, contract 24e8e6e3). Three "
    "example consumers in a left column each pin tag v0.5.0 to it: Engram/Paper2Brain (example commander), "
    "crebain (example body), pid_vla (example observer client). A separate pid-rs node (PID estimators "
    "science library) links to pid_vla by a distinct dashed grey edge labelled 'git submodule · NOT an NCP "
    "wire consumer' and does not connect to the contract.")


# ───────────────────────────── 3. VERSIONING ─────────────────────────────
def versioning(th):
    W, H = 820, 520
    s = [svg_open(W, H), defs(th), background(th, W, H)]
    s.append(title_block(th, "VERSION HANDSHAKE",
                         "COMPATIBILITY GATE  ·  HARD FAIL-CLOSED  ·  EXACT MAJOR.MINOR", W))
    s.append(sheet_meta(th, W - 28, 48, "NCP · WIRE 0.5 · CONTRACT 24e8e6e31e1dec8a"))
    ctr, verm, grn, ctl, obs = th["contract"], th["action"], th["active"], th["control"], th["observation"]

    # ---- edges (painted under cards) ----
    s.append(path("M276,260 L364,260", stroke=ctl, sw=2.5, marker="arrowControl"))
    for tx in (300, 324, 348):
        s.append(line(tx, 256, tx, 264, ctl, 1, op=0.7))
    # reject fork (up)
    s.append(path("M516,260 L532,260 Q540,260 540,252 L540,154 Q540,146 548,146 L568,146",
                  stroke=verm, sw=3, marker="arrowEstop"))
    # accept fork (down) — the ONE halo
    s.append(line(540, 268, 540, 350, grn, 8, op=th["halo_op"], filt="halo"))
    s.append(path("M516,260 L532,260 Q540,260 540,268 L540,342 Q540,350 548,350 L568,350",
                  stroke=grn, sw=3, marker="arrowActive"))
    # advisory drop (dashed)
    s.append(path("M678,400 L678,432", stroke=obs, sw=1.5, dash="3 3", marker="tapObserve"))

    # ---- N1 WIRE-BREAK ----
    bx, by, bw, bh = 56, 196, 220, 128
    s.append(card(th, bx, by, bw, bh, ctr, "S0"))
    s.append(ic_break(bx + 16, by + 38, 24, ctr))
    s.append(T(bx + 48, by + 46, "WIRE 0.4 → 0.5", 14, 700, th["tprim"]))
    s.append(T(bx + 48, by + 61, "string → enum", 10.5, 500, th["tsec"]))
    s.append(T(bx + 18, by + 86, "buf WIRE / WIRE_JSON", 9.5, 500, th["tmut"], mono=True))
    s.append(T(bx + 18, by + 103, "2cf0763ad61e4f1c →", 9.5, 500, th["tmut"], mono=True))
    s.append(T(bx + 18, by + 116, "24e8e6e31e1dec8a", 9.5, 700, ctr, mono=True))

    # ---- N2 GATE (diamond) ----
    s.append(diamond(440, 260, 76, th, ctr))
    s.append(ic_key(426, 212, 26, ctr))
    s.append(T(440, 252, "check_version", 12.5, 700, th["tprim"], mono=True, anchor="middle"))
    s.append(T(440, 268, "HARD", 9.5, 700, ctr, anchor="middle", track=0.6))
    s.append(T(440, 281, "exact major.minor", 9.5, 600, th["tsec"], anchor="middle"))
    s.append(T(440, 294, "FAIL-CLOSED", 9.5, 700, verm, anchor="middle", track=0.6))

    # ---- N3 REJECT ----
    rx_, ry, rw, rh = 568, 96, 220, 100
    s.append(card(th, rx_, ry, rw, rh, verm, "R0"))
    s.append(ic_noentry(rx_ + rw - 40, ry + 8, 22, verm))
    s.append(rect(rx_ + 18, ry + 40, 9, 9, rx=2, fill=verm))
    s.append(T(rx_ + 33, ry + 48, "REJECTED", 14, 700, th["tprim"]))
    s.append(T(rx_ + 18, ry + 66, "peer 0.4 ≠ 0.5", 10.5, 500, th["tsec"]))
    s.append(T(rx_ + 18, ry + 84, "fail-closed · Err · NO coerce", 9.5, 500, th["tmut"], mono=True))

    # ---- N4 ACCEPT (hero, green glow) ----
    ax, ay, aw, ah = 568, 300, 220, 100
    s.append(rect(ax, ay, aw, ah, rx=12, fill="url(#surface)", stroke="none", filt="glowActive"))
    s.append(rect(ax, ay, aw, ah, rx=12, fill="none", stroke=grn, sw=1.5))
    s.append(rect(ax + 6, ay + 12, 4, ah - 24, rx=2, fill=grn))
    s.append(reg_ticks(th, ax, ay, aw, ah))
    s.append(rect(ax + 14, ay + 10, 26, 15, rx=4, fill=th["surf_chip"], stroke=th["border"], sw=1))
    s.append(T(ax + 27, ay + 20.5, "A0", 10, 700, th["tsec"], mono=True, anchor="middle"))
    s.append(ic_play(ax + aw - 40, ay + 8, 22, grn))
    s.append(T(ax + 18, ay + 50, "SESSION OPENS", 14, 700, th["tprim"]))
    s.append(T(ax + 18, ay + 68, "peer 0.5 → Ok", 10.5, 500, th["tsec"]))
    s.append(T(ax + 18, ay + 86, "exact (major, minor) match", 9.5, 500, th["tmut"], mono=True))

    # ---- N5 ADVISORY ----
    vx, vy, vw, vh = 568, 432, 220, 56
    s.append(rect(vx, vy, vw, vh, rx=10, fill="url(#surface)", stroke="none", filt="soft"))
    s.append(rect(vx, vy, vw, vh, rx=10, fill="none", stroke=th["border"], sw=1.5))
    s.append(rect(vx + 6, vy + 10, 4, vh - 20, rx=2, fill=obs, op=0.7))
    s.append(rect(vx + 14, vy + 9, 26, 15, rx=4, fill=th["surf_chip"], stroke=th["border"], sw=1))
    s.append(T(vx + 27, vy + 19.5, "H0", 10, 700, th["tmut"], mono=True, anchor="middle"))
    s.append(ic_approx(vx + 46, vy + 16, 18, obs))
    s.append(T(vx + 70, vy + 25, "contract_hash diff", 12, 600, th["tprim"], mono=True))
    s.append(T(vx + 70, vy + 40, "ADVISORY · logged, not rejected", 9.5, 500, th["tmut"], italic=True))

    # ---- edge chips ----
    def echip(cx, cy, sq_hue, eyebrow, key, w=150):
        h = 22
        x, y = cx - w / 2, cy - h / 2
        return (rect(x - 1.5, y - 1.5, w + 3, h + 3, rx=8, fill=th["bg_bot"])
                + rect(x, y, w, h, rx=8, fill=th["surf_chip"], stroke=th["border"], sw=1)
                + rect(x + 7, y + 6, 10, 10, rx=2, fill=sq_hue)
                + T(x + 22, y + 15, eyebrow, 9.5, 700, sq_hue)
                + T(x + 22 + len(eyebrow) * 6.3 + 8, y + 15, key, 9, 500, th["tmut"], mono=True))
    s.append(echip(320, 260, ctl, "OPEN", "negotiate(peer)", w=170))
    s.append(echip(605, 122, verm, "0.4", "exact major.minor", w=168))
    s.append(echip(605, 326, grn, "0.5", "(major, minor) match", w=176))

    # ---- legend ----
    ly = 500
    items = [(verm, "■", "HARD", "fail-closed · Err, no coerce"),
             (grn, "▶", "OPEN", "exact 0.5 match → session"),
             (obs, "≈", "ADVISORY", "logged, not rejected")]
    lx = 40
    for hue, gly, concept, tail in items:
        s.append(T(lx, ly + 4, gly, 10, 700, hue))
        s.append(T(lx + 14, ly + 4, concept, 10, 700, hue, track=0.4))
        s.append(T(lx + 14 + len(concept) * 7 + 6, ly + 4, tail, 9, 500, th["tmut"]))
        lx += 248
    s.append("</svg>")
    return "".join(s)


VERSIONING_ALT = ("NCP version-compatibility handshake. The wire contract breaks from 0.4 to 0.5 (a "
    "string-to-enum change under buf WIRE/WIRE_JSON; contract hash 2cf0763ad61e4f1c becomes "
    "24e8e6e31e1dec8a). This feeds a hard compatibility gate, check_version, which requires an exact "
    "major.minor match and fails closed. A peer on 0.4 does not equal 0.5 and is rejected fail-closed "
    "with an error and no coercion; a peer on 0.5 matches exactly and the session opens (the highlighted "
    "green outcome). Separately, off the success path, a contract_hash difference is advisory only — "
    "logged, not rejected.")


# ───────────────────────────── 4. SAFETY FSM ─────────────────────────────
def fsm(th):
    W, H = 820, 544
    s = [svg_open(W, H), defs(th), background(th, W, H)]
    s.append(title_block(th, "SAFETY GOVERNOR · FSM",
                         "PLANT-SIDE STATE MACHINE  ·  FAIL-SAFE TO ZERO  ·  ESTOP LATCHES", W))
    s.append(sheet_meta(th, W - 28, 48, "NCP · WIRE 0.5 · SHEET 04/05"))
    grn, amb, verm, pink, obs = th["active"], th["hold"], th["action"], th["configfail"], th["observation"]
    ink = "#0d1117" if th["name"] == "dark" else "#1b2733"

    # mode-enum ribbon (top-right, under sheet-meta)
    rx0 = W - 28
    for label, fill, txt, filled in reversed([("init", None, th["tsec"], th["surf_chip"]),
            ("active", grn, "#06281e" if th["name"] == "dark" else "#fff", True),
            ("hold", amb, amb, False), ("estop", verm, "#fff", True)]):
        pw = 10 + len(label) * 6.2
        rx0 -= pw
        if filled is True:
            s.append(rect(rx0, 60, pw, 18, rx=6, fill=fill))
        elif filled is None:
            s.append(rect(rx0, 60, pw, 18, rx=6, fill=txt, stroke=th["border"], sw=1)); txt = th["tsec"]
        else:
            s.append(rect(rx0, 60, pw, 18, rx=6, fill="none", stroke=fill, sw=1.2))
        s.append(T(rx0 + pw / 2, 72.5, label, 9, 700, txt, anchor="middle", mono=True))
        rx0 -= 7

    def klabel(cx, cy, trigger, eyebrow=None, ehue=None, compact=False):
        if compact and eyebrow:
            w = round(len(eyebrow) * 6.0 + len(trigger) * 5.3 + 24)
            x, y = cx - w / 2, cy - 9
            return (rect(x, y, w, 18, rx=6, fill=th["bg_bot"], op=0.92, stroke=th["border"], sw=0.8)
                    + T(x + 8, y + 13, eyebrow, 9, 700, ehue)
                    + T(round(x + 8 + len(eyebrow) * 6.0 + 6), y + 13, trigger, 9, 500, th["tmut"], mono=True))
        w = round(max(len(trigger) * 5.3, (len(eyebrow) * 6.3 if eyebrow else 0)) + 16)
        h = 30 if eyebrow else 18
        x, y = cx - w / 2, cy - h / 2
        o = [rect(x, y, w, h, rx=6, fill=th["bg_bot"], op=0.92, stroke=th["border"], sw=0.8)]
        ty = y + 13
        if eyebrow:
            o.append(T(x + 8, ty, eyebrow, 9.5, 700, ehue, track=0.4)); ty += 13
        o.append(T(x + 8, ty, trigger, 9, 500, th["tmut"], mono=True))
        return "".join(o)

    # ---- state geometry ----
    AC = (96, 150, 200, 72)    # ACTIVE  (96-296, 150-222) cy186
    HD = (430, 150, 200, 72)   # HOLD    (430-630)
    ES = (430, 330, 212, 86)   # ESTOP   (430-642, 330-416) cy373  HERO
    CF = (96, 330, 200, 72)    # CONFIG-FAIL-CLOSED

    # ---- edges (painted first) ----
    s.append(line(110, 124, 110, 150, obs, 2, marker="arrowMut"))            # E0 INIT→ACTIVE
    s.append(path("M214,150 C214,120 250,120 250,150", stroke=grn, sw=2.5, marker="arrowActive"))  # E1 self
    s.append(path("M296,172 L430,172", stroke=amb, sw=2.5, marker="arrowHold"))   # E2 ACTIVE→HOLD
    s.append(path("M430,200 L296,200", stroke=grn, sw=2.5, marker="arrowActive"))  # E3 HOLD→ACTIVE
    # E4 ACTIVE→ESTOP (hero: halo + 4px busAction)
    s.append(path("M296,206 L360,206 Q368,206 368,214 L368,360 Q368,368 376,368 L430,368",
                  stroke=verm, sw=9, op=th["halo_op"], filt="halo"))
    s.append(path("M296,206 L360,206 Q368,206 368,214 L368,360 Q368,368 376,368 L430,368",
                  stroke="url(#busAction)", sw=4, marker="arrowEstop"))
    for ty in (250, 300, 350):
        s.append(line(363, ty, 373, ty, verm, 1, op=0.7))
    s.append(path("M530,222 L530,330", stroke=verm, sw=3.5, marker="arrowEstop"))  # E5 HOLD→ESTOP
    s.append(path("M642,356 C680,356 680,392 642,392", stroke=verm, sw=3.5, marker="arrowEstop"))  # E6 self latched
    # E7 ESTOP→ACTIVE recover (operator-gated, dashed green, long way round bottom-left)
    s.append(path("M470,416 L470,470 Q470,478 462,478 L72,478 Q64,478 64,470 L64,194 Q64,186 72,186 L96,186",
                  stroke=grn, sw=2, dash="5 4", marker="arrowActive"))
    s.append(path("M150,222 L150,330", stroke=pink, sw=2, dash="5 3", marker="arrowMut"))  # E8 ACTIVE→CONFIG
    s.append(path("M188,402 C224,402 224,434 188,434", stroke=pink, sw=2, dash="5 3", marker="arrowMut"))  # E9 self

    # ---- state cards ----
    s.append(card(th, *AC, grn, "S1"))
    s.append(ic_play(AC[0] + AC[2] - 38, AC[1] + 9, 22, grn))
    s.append(T(AC[0] + 18, AC[1] + 34, "ACTIVE", 14, 700, th["tprim"]))
    s.append(T(AC[0] + 18, AC[1] + 50, "nominal · command authority", 10, 500, th["tsec"]))
    s.append(T(AC[0] + 18, AC[1] + 64, "Mode::active", 9, 500, th["tmut"], mono=True))

    s.append(card(th, *HD, amb, "S2"))
    s.append(ic_pause(HD[0] + HD[2] - 38, HD[1] + 9, 22, amb))
    s.append(T(HD[0] + 18, HD[1] + 34, "HOLD", 14, 700, th["tprim"]))
    s.append(rect(HD[0] + 70, HD[1] + 24, 88, 15, rx=7, fill=th["surf_chip"], stroke=amb, sw=1))
    s.append(T(HD[0] + 114, HD[1] + 34.5, "NON-LATCHING", 8, 700, amb, anchor="middle"))
    s.append(T(HD[0] + 18, HD[1] + 50, "self-clears on fresh data", 10, 500, th["tsec"]))
    s.append(T(HD[0] + 18, HD[1] + 64, "ZEROED frame · Mode::hold", 9, 500, th["tmut"], mono=True))

    s.append(card(th, *CF, pink, "S4", dashed=True))
    s.append(ic_warn(CF[0] + CF[2] - 38, CF[1] + 9, 22, pink))
    s.append(T(CF[0] + 18, CF[1] + 32, "CONFIG-FAIL-CLOSED", 12.5, 700, th["tprim"]))
    s.append(rect(CF[0] + 18, CF[1] + 40, 96, 15, rx=7, fill=th["surf_chip"], stroke=pink, sw=1))
    s.append(T(CF[0] + 66, CF[1] + 50.5, "safety_ok=false", 8, 700, pink, anchor="middle", mono=True))
    s.append(T(CF[0] + 122, CF[1] + 51, "permanent", 9.5, 500, th["tsec"]))
    s.append(T(CF[0] + 18, CF[1] + 65, "reset() does NOT clear", 9, 500, th["tmut"], mono=True))

    # ESTOP hero (filled vermillion + glow + 4 corner lock-ticks)
    ex, ey, ew, eh = ES
    s.append(rect(ex, ey, ew, eh, rx=10, fill="url(#busAction)", stroke="#ffd9c2", sw=2, filt="glow"))
    for lx, ly, dx, dy in [(ex, ey, 1, 1), (ex + ew, ey, -1, 1), (ex, ey + eh, 1, -1), (ex + ew, ey + eh, -1, -1)]:
        s.append(path(f"M{lx + 9 * dx},{ly} L{lx},{ly} L{lx},{ly + 9 * dy}", stroke="#ffd9c2", sw=1.6, op=0.9))
    s.append(rect(ex + 14, ey + 11, 26, 15, rx=4, fill=th["surf_chip"]))
    s.append(T(ex + 27, ey + 21.5, "S3", 10, 700, th["tsec"], mono=True, anchor="middle"))
    s.append(ic_octagon(ex + ew - 40, ey + 10, 24, ink))
    s.append(T(ex + 18, ey + 44, "ESTOP", 15, 800, ink))
    s.append(T(ex + 18, ey + 61, "LATCHED · de-energized", 10.5, 600, ink))
    s.append(T(ex + 18, ey + 77, "exits only via supervisor reset()", 9.5, 500, ink, mono=True))

    # ---- edge labels ----
    s.append(klabel(235, 116, "clamp speed · truncate horizon", "fresh sensor", grn, compact=True))
    s.append(klabel(363, 172, "stale · NaN · timeout", "↓ HOLD", amb, compact=True))
    s.append(klabel(363, 200, "fresh in-bounds", "↑ recover", grn, compact=True))
    s.append(klabel(338, 285, "geofence · NaN pos · burst", "BREACH", verm, compact=True))
    s.append(klabel(560, 278, "geofence · link burst", "BREACH", verm, compact=True))
    s.append(klabel(726, 374, "every frame zeroed", "LATCHED", verm, compact=True))
    s.append(klabel(286, 478, "supervisor reset() then in-bounds", "reset()", grn, compact=True))
    s.append(klabel(188, 282, "undeclared channel", "MISCONFIG", pink, compact=True))

    # ---- invariant band ----
    iy = 496
    s.append(rect(28, iy, W - 56, 26, rx=8, fill=th["surf_chip"], stroke=th["border"], sw=1))
    s.append(rect(28, iy, 4, 26, rx=2, fill=verm))
    s.append(T(44, iy + 16.5, "INVARIANT · HOLD / ESTOP / CONFIG-FAIL-CLOSED all emit a ZEROED frame — fail-safe to zero, NOT latch-last.  ESTOP latches until reset();  CONFIG-FAIL-CLOSED is permanent (safety_ok=false).",
               9, 500, th["tsec"], italic=True))
    s.append("</svg>")
    return "".join(s)


FSM_ALT = ("NCP plant-side safety governor finite state machine. Four states: ACTIVE (nominal — clamps "
    "speed and truncates the predictive horizon near the geofence), HOLD (non-latching — self-clears on "
    "fresh in-bounds data), ESTOP (latched and de-energized — exits only via a supervisor reset(); the "
    "emphasized vermillion glowing state with corner lock-ticks), and CONFIG-FAIL-CLOSED (a limit cites an "
    "undeclared channel; permanent for the session, safety_ok=false, reset() does not clear it). "
    "Transitions: INIT to ACTIVE; ACTIVE self-loops on a fresh sensor; ACTIVE to HOLD on a stale or missing "
    "sensor, non-finite clock or velocity, bad timeout, or absent geofence channel; HOLD back to ACTIVE on "
    "fresh in-bounds data; ACTIVE and HOLD both latch to ESTOP on a geofence breach, non-finite position, "
    "or link-loss burst (the heaviest strokes); ESTOP self-loops while latched with every CommandFrame "
    "zeroed, returning to ACTIVE only after a supervisor reset() with the plant in bounds; ACTIVE enters "
    "CONFIG-FAIL-CLOSED when a limit references an undeclared channel, then self-loops. Invariant: HOLD, "
    "ESTOP, and CONFIG-FAIL-CLOSED all emit a ZEROED command frame — fail-safe to zero, not latch-last.")


# ───────────────────────────── 5. SEQUENCE ─────────────────────────────
def sequence(th):
    W, H = 820, 640
    s = [svg_open(W, H), defs(th), background(th, W, H)]
    s.append(title_block(th, "SESSION LIFECYCLE",
                         "CLIENT ⇄ SERVER  ·  OPEN → STEP / OBSERVE → CLOSE", W))
    s.append(sheet_meta(th, W - 28, 48, "NCP · WIRE 0.5 · proto/ncp.proto"))
    ctl, obs, ctr, verm, grn, pink = (th["control"], th["observation"], th["contract"],
                                      th["action"], th["active"], th["configfail"])
    CLx, SVx = 246, 574

    # phase-group frames (recessive wells)
    for fy, fh, tag, thue, note in [(176, 124, "OPEN", ctl, "HARD version gate · ADVISORY hash"),
                                    (320, 168, "loop  [per chunk]", verm, "step ⟳ observe · provenance every frame"),
                                    (508, 92, "CLOSE", ctl, "teardown")]:
        s.append(rect(210, fy, 400, fh, rx=10, fill=th["surf_chip"], op=0.32, stroke=th["border"], sw=0.8, dash="2 3"))
        tw = 18 + len(tag) * 6.0
        s.append(rect(210, fy, tw, 18, rx=6, fill=thue))
        s.append(T(218, fy + 13, tag, 9.5, 700, "#ffffff" if tag.startswith("loop") else "#ffffff", mono=True))
        s.append(T(210 + tw + 10, fy + 13, note, 9, 500, th["tmut"], italic=True))

    # lifelines
    s.append(line(CLx, 156, CLx, 604, th["border"], 1.5, dash="4 4", op=0.7))
    s.append(line(SVx, 156, SVx, 604, th["border"], 1.5, dash="4 4", op=0.7))
    # activation bars per phase
    for ay, ah in [(208, 92), (360, 124), (532, 64)]:
        for lx in (CLx, SVx):
            s.append(rect(lx - 5, ay, 10, ah, rx=3, fill=th["surf_chip"], stroke=th["border"], sw=1))

    # actor cards
    s.append(card(th, 156, 92, 180, 64, ctl, "C0"))
    s.append(ic_terminal(300, 104, 22, ctl))
    s.append(T(200, 120, "CLIENT", 14, 700, th["tprim"]))
    s.append(T(200, 136, "commander · opens + drives", 10, 500, th["tsec"]))
    s.append(card(th, 484, 92, 180, 64, obs, "S0"))
    s.append(ic_wave(628, 104, 22, obs))
    s.append(T(528, 120, "SERVER", 14, 700, th["tprim"]))
    s.append(T(528, 136, "sim backend · is_simulation_output", 9, 500, th["tsec"], mono=True))

    # message chip helper (2-line: eyebrow + mono key)
    def mchip(cx, cy, desig, hue, eyebrow, key, w):
        x, y = cx - w / 2, cy - 15
        return (rect(x - 1.5, y - 1.5, w + 3, 33, rx=8, fill=th["bg_bot"])
                + rect(x, y, w, 30, rx=8, fill=th["surf_chip"], stroke=th["border"], sw=1)
                + rect(x + 7, y + 7, 16, 16, rx=3, fill=hue)
                + T(x + 15, y + 18.5, desig, 8.5, 700, "#ffffff", mono=True, anchor="middle")
                + T(x + 30, y + 13, eyebrow, 9.5, 700, hue, track=0.4)
                + T(x + 30, y + 24, key, 8.5, 500, th["tmut"], mono=True))

    # OPEN gate note on SERVER lifeline
    s.append(rect(486, 230, 150, 44, rx=10, fill="url(#surface)", stroke=th["border"], sw=1.25, filt="soft"))
    s.append(rect(486 + 6, 230 + 8, 4, 28, rx=2, fill=ctr))
    s.append(T(498, 246, "check_version  HARD/fail-closed", 8.5, 500, th["tmut"], mono=True))
    s.append(T(498, 262, "contract_hash  ADVISORY/logged", 8.5, 500, th["tmut"], mono=True))

    # E1 OpenSession →
    s.append(line(251, 214, 569, 214, ctl, 2.5, marker="arrowControl"))
    s.append(mchip(410, 214, "C1", ctl, "OpenSession  →", "ncp_version · contract_hash · network · …", 300))
    # E2 SessionOpened ← (+ outcome pills)
    s.append(line(569, 288, 251, 288, ctl, 2.5, dash="4 4", marker="replyControl"))
    s.append(mchip(410, 288, "C1", ctl, "SessionOpened  ←", "ok · backend · resolved · provenance", 282))
    s.append(rect(282, 302, 132, 16, rx=6, fill=grn))
    s.append(T(348, 313, "ok=true → opens", 8.5, 700, "#06281e" if th["name"] == "dark" else "#ffffff", anchor="middle", mono=True))
    s.append(rect(420, 302, 150, 16, rx=6, fill="none", stroke=pink, sw=1.2))
    s.append(T(495, 313, "ok=false → NO session", 8.5, 700, pink, anchor="middle", mono=True))
    # E3 StepRequest →
    s.append(line(251, 372, 569, 372, ctl, 2.5, marker="arrowControl"))
    s.append(mchip(410, 372, "C1", ctl, "StepRequest / RunRequest  →", "advance_ms (0⇒chunk_ms) · stimulus", 300))

    # E4 ObservationFrame ← (HERO)
    s.append(line(569, 432, 251, 432, verm, 9, op=th["halo_op"], filt="halo"))
    s.append(line(569, 432, 251, 432, "url(#busAction)", 4, marker="arrowAction"))
    for tx in (320, 410, 500):
        s.append(line(tx, 427, tx, 437, verm, 1, op=0.7))
    cw, ch, cy0 = 312, 38, 414
    cx0 = 410 - cw / 2
    s.append(rect(cx0 - 1.5, cy0 - 1.5, cw + 3, ch + 3, rx=10, fill=th["bg_bot"]))
    s.append(rect(cx0, cy0, cw, ch, rx=10, fill="url(#surface)", stroke=verm, sw=1.5, filt="glow"))
    s.append(rect(cx0, cy0, cw, ch, rx=10, fill=verm, op=th["wash_op"]))
    s.append(rect(cx0 + 8, cy0 + 8, 16, 16, rx=3, fill=verm))
    s.append(T(cx0 + 16, cy0 + 19.5, "O1", 8.5, 700, "#ffffff", mono=True, anchor="middle"))
    s.append(T(cx0 + 30, cy0 + 15, "ObservationFrame  ←", 10, 700, verm, track=0.3))
    s.append(T(cx0 + 30, cy0 + 27, "seq · t · sim_time_ms · records{…}", 8.5, 500, th["tsec"], mono=True))
    # provenance invariant pills
    py = cy0 + ch + 5
    s.append(rect(254, py, 160, 17, rx=6, fill=grn))
    s.append(T(334, py + 12, "is_simulation_output = true", 8.5, 700, "#06281e" if th["name"] == "dark" else "#ffffff", anchor="middle", mono=True))
    s.append(rect(420, py, 168, 17, rx=6, fill=pink))
    s.append(T(504, py + 12, "calibrated_posterior = false", 8.5, 700, "#3a1029" if th["name"] == "dark" else "#ffffff", anchor="middle", mono=True))
    s.append(T(410, py + 30, "fixed provenance invariants on every frame — the honesty boundary", 9, 500, th["tmut"], italic=True, anchor="middle"))

    # E5 CloseSession →  / E6 SessionClosed ←
    s.append(line(251, 540, 569, 540, ctl, 2.5, marker="arrowControl"))
    s.append(mchip(410, 540, "C1", ctl, "CloseSession  →", "session_id", 200))
    s.append(line(569, 580, 251, 580, ctl, 2.5, dash="4 4", marker="replyControl"))
    s.append(mchip(410, 580, "C1", ctl, "SessionClosed  ←", "ok=true", 180))

    # legend
    ly = 618
    s.append(line(40, ly, 64, ly, ctl, 2.5, marker="arrowControl"))
    s.append(T(72, ly + 4, "CONTROL · request/reply", 9, 600, th["tsec"]))
    s.append(line(300, ly, 324, ly, "url(#busAction)", 4, marker="arrowAction"))
    s.append(T(332, ly + 4, "ObservationFrame · provenance-bearing (hero)", 9, 600, th["tsec"]))
    s.append("</svg>")
    return "".join(s)


SEQUENCE_ALT = ("NCP session-lifecycle sequence diagram. Two lifelines: CLIENT (commander) and SERVER (sim "
    "backend). Three grouped phases top to bottom. OPEN: CLIENT sends OpenSession (ncp_version, "
    "contract_hash, network, record, stimulus, sim); SERVER applies a HARD version gate (check_version, "
    "exact major.minor, fail-closed) plus an ADVISORY contract_hash compare, then replies SessionOpened — "
    "ok=true opens the session (backend, resolved, provenance, contract_hash), ok=false returns an error "
    "with no session. STEP/OBSERVE loop, once per chunk: CLIENT sends StepRequest or RunRequest (advance_ms, "
    "0 means use chunk_ms, with a stimulus); SERVER replies ObservationFrame (seq, t, sim_time_ms, records) "
    "— the heaviest, glowing vermillion trace, because it asserts two fixed provenance invariants on every "
    "frame: is_simulation_output=true and calibrated_posterior=false (the honesty boundary). CLOSE: CLIENT "
    "sends CloseSession; SERVER replies SessionClosed ok=true. Wire 0.5, contract hash 24e8e6e31e1dec8a.")


DIAGRAMS = {
    "topology": (topology, TOPOLOGY_ALT),
    "ecosystem": (ecosystem, ECOSYSTEM_ALT),
    "versioning": (versioning, VERSIONING_ALT),
    "fsm": (fsm, FSM_ALT),
    "sequence": (sequence, SEQUENCE_ALT),
}


def main():
    outdir = os.path.join("docs", "diagrams")
    os.makedirs(outdir, exist_ok=True)
    for name, (fn, _alt) in DIAGRAMS.items():
        for th in (LIGHT, DARK):
            svg = fn(th)
            p = os.path.join(outdir, f"{name}-{th['name']}.svg")
            with open(p, "w") as f:
                f.write(svg)
            print(f"wrote {p}  ({len(svg)} bytes)")


if __name__ == "__main__":
    main()
