"""Build the progress page from AGENTS.md and the git history.

Reads the weighted progress table (section 6.0), the overall figure of every
commit that touched AGENTS.md, the agent lanes in tools/lanes.json and a few
repository counts, and writes <out>/index.html and <out>/progress.json.

    py tools/progress.py --out site
"""

import argparse
import html
import json
import re
import subprocess
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
AGENTS = ROOT / "AGENTS.md"
LANES = ROOT / "tools" / "lanes.json"
OVERALL = re.compile(r"\*\*Overall:\s*~?([\d.]+)%")
ROW = re.compile(r"^\|\s*(.+?)\s*\|\s*(\d+)%\s*\|\s*~?([\d.]+)%\s*\|\s*(.*?)\s*\|\s*$")
LOG_ENTRY = re.compile(r"^>?- \*\*(.+?)\*\*:?\s*(.*)$")


def git(*args):
    return subprocess.run(["git", *args], cwd=ROOT, capture_output=True, text=True, encoding="utf-8", errors="replace").stdout


def section(text, start, end):
    a = text.find(start)
    if a < 0:
        return ""
    b = text.find(end, a + len(start))
    return text[a:b if b >= 0 else len(text)]


def parts(text):
    rows = []
    for line in section(text, "## 6.0", "\n## ").splitlines():
        m = ROW.match(line)
        if m:
            rows.append({"name": m[1], "weight": int(m[2]), "done": float(m[3]), "comment": m[4]})
    return rows


def history():
    points = []
    for line in git("log", "--format=%H %cI", "--", "AGENTS.md").splitlines():
        sha, when = line.split(" ", 1)
        m = OVERALL.search(git("show", f"{sha}:AGENTS.md"))
        if m:
            points.append({"time": when, "overall": float(m[1]), "commit": sha[:7]})
    points.reverse()
    return points


def log_entries(text, limit):
    entries = []
    for line in section(text, "## 7. Log", "\n## ").splitlines():
        m = LOG_ENTRY.match(line.strip())
        if m:
            entries.append({"title": m[1], "text": m[2]})
    entries.sort(key=lambda e: e["title"], reverse=True)
    return entries[:limit]


def counts():
    rust = list((ROOT / "crates").rglob("*.rs"))
    tests = sum(len(re.findall(r"#\[test\]", p.read_text(encoding="utf-8", errors="replace"))) for p in rust)
    lines = sum(p.read_text(encoding="utf-8", errors="replace").count("\n") for p in rust)
    return {
        "rust_lines": lines,
        "tests": tests,
        "crates": len([p for p in (ROOT / "crates").iterdir() if (p / "Cargo.toml").exists()]),
        "specs": len(list((ROOT / "docs" / "spec").glob("*.md"))),
        "unicorn_harnesses": len(list((ROOT / "tools" / "emu").glob("*_emu.py"))),
        "commits": int(git("rev-list", "--count", "HEAD").strip() or 0),
    }


def inline(text):
    text = html.escape(text, quote=False)
    text = re.sub(r"`([^`]+)`", r"<code>\1</code>", text)
    return re.sub(r"\*\*([^*]+)\*\*", r"<b>\1</b>", text)


def chart(points):
    if len(points) < 2:
        return "<p class=sub>Пока мало истории.</p>"
    w, h, left, right, top, bottom = 760, 170, 40, 12, 10, 24
    t = [datetime.fromisoformat(p["time"]).timestamp() for p in points]
    t0, t1 = t[0], t[-1] if t[-1] > t[0] else t[0] + 1

    def x(v):
        return left + (v - t0) / (t1 - t0) * (w - left - right)

    def y(v):
        return top + (1 - v / 100) * (h - top - bottom)

    grid = "".join(
        f'<line x1="{left}" x2="{w - right}" y1="{y(v):.1f}" y2="{y(v):.1f}" class="grid"/>'
        f'<text x="{left - 6}" y="{y(v) + 4:.1f}" class="tick" text-anchor="end">{v}%</text>'
        for v in (0, 25, 50, 75, 100))
    path = "M" + " L".join(f"{x(a):.1f},{y(p['overall']):.1f}" for a, p in zip(t, points))
    first = datetime.fromtimestamp(t0).strftime("%d %b")
    last = datetime.fromtimestamp(t1).strftime("%d %b %H:%M")
    data = json.dumps([[round(x(a), 1), round(y(p["overall"]), 1), p["overall"], p["time"][:16].replace("T", " "), p["commit"]]
                       for a, p in zip(t, points)])
    return f"""<div class="chart"><svg viewBox="0 0 {w} {h}" role="img" aria-label="Системы разобраны, динамика" id="hist">
{grid}<path d="{path}" class="line"/>
<circle r="5" class="dot" id="dot" cx="{x(t[-1]):.1f}" cy="{y(points[-1]['overall']):.1f}"/>
<line id="cross" class="cross" y1="{top}" y2="{h - bottom}" x1="-10" x2="-10"/>
<text x="{left}" y="{h - 6}" class="tick">{first}</text><text x="{w - right}" y="{h - 6}" class="tick" text-anchor="end">{last}</text>
</svg><div id="tip" class="tip" hidden></div></div>
<script>
(() => {{
  const pts = {data}, svg = document.getElementById('hist'), tip = document.getElementById('tip'),
        dot = document.getElementById('dot'), cross = document.getElementById('cross');
  svg.addEventListener('pointermove', e => {{
    const r = svg.getBoundingClientRect(), sx = (e.clientX - r.left) * {w} / r.width;
    let best = pts[0];
    for (const p of pts) if (Math.abs(p[0] - sx) < Math.abs(best[0] - sx)) best = p;
    dot.setAttribute('cx', best[0]); dot.setAttribute('cy', best[1]);
    cross.setAttribute('x1', best[0]); cross.setAttribute('x2', best[0]);
    tip.hidden = false;
    tip.innerHTML = '<b>' + best[2] + '%</b><br>' + best[3] + '<br><code>' + best[4] + '</code>';
    tip.style.left = Math.min(best[0] / {w} * r.width + 12, r.width - 150) + 'px';
    tip.style.top = (best[1] / {h} * r.height - 10) + 'px';
  }});
  svg.addEventListener('pointerleave', () => {{ tip.hidden = true; cross.setAttribute('x1', -10); cross.setAttribute('x2', -10); }});
}})();
</script>"""


PART_NAMES = [
    ("File formats", "Форматы файлов"), ("Game.dll analysis", "Разбор Game.dll"), ("Simulation", "Симуляция"),
    ("JASS VM", "JASS и нативные функции"), ("Rendering", "Графика"), ("UI", "Интерфейс"),
    ("Network", "Сеть, wasm, звук"), ("Replay test harness", "Сверка с реплеями"),
]
STATUS = {"running": "работает", "done": "готово", "failed": "не вышло"}


def short_name(name):
    return next((ru for en, ru in PART_NAMES if name.startswith(en)), name)


def behaviour():
    """Behaviour metrics: how much already acts like the original, computed from the repository."""
    def code_lines(text):
        # A code only listed without behaviour (status `Listed`, or a table of listed-only order ids) does not count.
        out, skip = [], False
        for line in text.splitlines():
            if "_LISTED" in line and "&[" in line:
                skip = True
            if skip:
                skip = not line.strip().startswith("];")
                continue
            if line.rstrip().endswith("Listed),"):
                continue
            out.append(line)
        return "\n".join(out)
    sim = "\n".join(code_lines(p.read_text(encoding="utf-8", errors="replace"))
                    for p in (ROOT / "crates" / "sim" / "src").rglob("*.rs"))
    codes = [l.split("\t")[0] for l in (ROOT / "docs" / "spec" / "ability_factory_1.26a.tsv").read_text(encoding="utf-8").splitlines()
             if l.strip() and not l.startswith("#")]
    abilities = sum(1 for c in codes if re.search("[\"']" + re.escape(c) + "[\"']", sim))
    natives = [l.split("\t")[0] for l in (ROOT / "docs" / "spec" / "natives-1.26a.tsv").read_text(encoding="utf-8").splitlines()
               if l.strip() and not l.startswith("#")]
    pending = (ROOT / "crates" / "sim" / "src" / "natives" / "pending.rs").read_text(encoding="utf-8", errors="replace")
    not_impl = set(re.findall(r'"(\w+)",?\s*support:\s*Support::NotImplemented', pending))
    code = "\n".join(p.read_text(encoding="utf-8", errors="replace") for d in ("sim", "jass")
                     for p in (ROOT / "crates" / d / "src").rglob("*.rs") if p.name != "pending.rs")
    bound = sum(1 for n in natives if f'"{n}"' in code and n not in not_impl)
    spec = "\n".join(p.read_text(encoding="utf-8", errors="replace") for p in (ROOT / "docs" / "spec").glob("*.md"))
    marks = {k: len(re.findall(r"\b" + k + r"\b", spec)) for k in ("CONFIRMED", "LIKELY", "UNSURE")}
    confirmed_abilities = 0
    confirmed_codes = []
    spec_lines = spec.splitlines()
    for c in codes:
        pat = re.compile(r"\b" + re.escape(c) + r"\b")
        if any(pat.search(l) and "CONFIRMED" in l and not re.search("UNSURE|LIKELY|NOT CONFIRMED|not CONFIRMED", l)
               for l in spec_lines):
            confirmed_abilities += 1
            confirmed_codes.append(c)
    listed = ROOT / "docs" / "spec" / "confirmed-abilities.tsv"
    if listed.exists():
        extra = {l.split(chr(9))[0] for l in listed.read_text(encoding="utf-8").splitlines() if l.strip() and not l.startswith("#")}
        confirmed_abilities = len({c for c in codes if c in extra} | set(confirmed_codes))
    replay = json.loads((ROOT / "tools" / "metrics.json").read_text(encoding="utf-8"))["replay"]
    rows = [
        {"name": "Способности: есть модель", "hint": "базовые способности, у которых в симуляции есть поведение (не обязательно точное)",
         "have": abilities, "total": len(codes), "weight": 10},
        {"name": "Способности: сверены с оригиналом", "hint": "в спецификациях есть строка CONFIRMED без LIKELY/UNSURE для этой способности",
         "have": confirmed_abilities, "total": len(codes), "weight": 25},
        {"name": "Сверка с реплеем", "hint": "записи контрольной суммы реплея, совпавшие точно",
         "have": replay["exact_pairs"], "total": replay["pairs"], "weight": 35},
        {"name": "Подтверждённые факты", "hint": "CONFIRMED среди CONFIRMED/LIKELY/UNSURE в спецификациях",
         "have": marks["CONFIRMED"], "total": sum(marks.values()), "weight": 20},
        {"name": "Нативные функции JASS", "hint": "функции скриптов, у которых есть реализация",
         "have": bound, "total": len(natives), "weight": 10},
    ]
    for r in rows:
        r["pct"] = 100.0 * r["have"] / r["total"] if r["total"] else 0.0
    score = round(sum(r["weight"] * r["pct"] for r in rows) / sum(r["weight"] for r in rows), 1)
    return {"score": score, "rows": rows, "marks": marks}


STYLE = """
:root{color-scheme:light;--bg:#f6f5f2;--card:#fff;--text:#16150f;--muted:#6b6960;--line:#e3e1da;--track:#ebe9e3;
--a:#2a78d6;--b:#8a877c;--ok:#1f8a4c;--warn:#b35c00}
@media (prefers-color-scheme:dark){:root{color-scheme:dark;--bg:#151514;--card:#1f1f1d;--text:#f2f1ec;--muted:#a3a197;
--line:#33322e;--track:#2c2b28;--a:#3987e5;--b:#8f8c82;--ok:#3fb26f;--warn:#e0a050}}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--text);font:14px/1.45 system-ui,-apple-system,"Segoe UI",sans-serif;font-variant-numeric:tabular-nums}
main{max-width:860px;margin:0 auto;padding:20px 16px 40px}
h1{font-size:18px;margin:0}h2{font-size:12px;font-weight:600;letter-spacing:.05em;text-transform:uppercase;color:var(--muted);margin:26px 0 10px}
.sub{color:var(--muted);font-size:12px;margin:2px 0 16px}.sub a{color:inherit}
.score{display:grid;grid-template-columns:1.4fr 1fr;gap:12px}
@media (max-width:560px){.score{grid-template-columns:1fr}}
.card{background:var(--card);border:1px solid var(--line);border-radius:12px;padding:14px 16px}
.card .k{font-size:12px;color:var(--muted)}.card .v{font-size:44px;font-weight:700;letter-spacing:-1px;line-height:1.1}
.card.sec .v{font-size:30px;color:var(--b)}.card .note{font-size:12px;color:var(--muted);margin-top:6px}
.bar{height:6px;background:var(--track);border-radius:3px;overflow:hidden;margin-top:8px}
.bar>i{display:block;height:100%;background:var(--a);border-radius:3px}.sec .bar>i,.parts .bar>i{background:var(--b)}
.rows{background:var(--card);border:1px solid var(--line);border-radius:12px;padding:2px 16px}
.row{display:grid;grid-template-columns:minmax(150px,1.1fr) 2fr 96px 34px;gap:12px;align-items:center;padding:9px 0;border-bottom:1px solid var(--line)}
.row:last-child{border-bottom:0}.row .bar{margin:0}.row .val{text-align:right;font-size:13px;line-height:1.25}
.row .w{text-align:right;color:var(--muted);font-size:11px}
.row .nm{font-weight:500}.row .nm small{display:block;font-weight:400;color:var(--muted);font-size:11px;line-height:1.3}
@media (max-width:560px){.row{grid-template-columns:1fr 80px}.row .bar{grid-column:1/-1;order:3}.row>.w{display:none}}
.chart{position:relative;background:var(--card);border:1px solid var(--line);border-radius:12px;padding:8px}
svg{width:100%;height:auto;display:block}.grid{stroke:var(--line);stroke-width:1}.tick{fill:var(--muted);font-size:11px}
.line{fill:none;stroke:var(--b);stroke-width:2;stroke-linejoin:round}.dot{fill:var(--b);stroke:var(--card);stroke-width:2}
.cross{stroke:var(--muted);stroke-width:1;stroke-dasharray:3 3}
.tip{position:absolute;background:var(--card);border:1px solid var(--line);border-radius:8px;padding:6px 10px;font-size:12px;pointer-events:none;box-shadow:0 2px 8px rgba(0,0,0,.15)}
.lanes{display:flex;flex-direction:column;gap:6px}
.lane{display:grid;grid-template-columns:44px 60px 1fr auto;gap:10px;align-items:baseline;background:var(--card);border:1px solid var(--line);border-radius:10px;padding:7px 12px;font-size:13px}
.lane .m{color:var(--muted);font-size:12px}
.st{font-size:11px;padding:1px 8px;border-radius:10px;border:1px solid var(--line);color:var(--muted);white-space:nowrap}
.st.running{color:var(--a);border-color:var(--a)}.st.done{color:var(--ok);border-color:var(--ok)}.st.failed{color:var(--warn);border-color:var(--warn)}
.stats{display:flex;flex-wrap:wrap;gap:6px 18px;color:var(--muted);font-size:13px}.stats b{color:var(--text)}
details{margin-top:20px}details summary{cursor:pointer;color:var(--muted);font-size:13px}
.log li{margin:0 0 8px;font-size:13px}.log b{white-space:nowrap}
code{font:12px ui-monospace,Consolas,monospace;background:var(--track);padding:0 4px;border-radius:4px}
"""


def bar(pct):
    return f"<div class=bar><i style='width:{min(max(pct, 0), 100):.1f}%'></i></div>"


def fmt(n):
    return f"{n:,}".replace(",", " ")


def page(data):
    b = data["behaviour"]
    brows = "".join(
        f"<div class=row><div class=nm>{r['name']}<small>{html.escape(r['hint'])}</small></div>{bar(r['pct'])}"
        f"<div class=val><b>{r['pct']:.1f}%</b><br><span class=w>{fmt(r['have'])} / {fmt(r['total'])}</span></div>"
        f"<div class=w>×{r['weight']}</div></div>" for r in b["rows"])
    prows = "".join(
        f"<div class=row title=\"{html.escape(re.sub('[`*]', '', p['comment']), quote=True)}\"><div class=nm>{short_name(p['name'])}</div>"
        f"{bar(p['done'])}<div class=val><b>{p['done']:g}%</b></div><div class=w>×{p['weight']}</div></div>" for p in data["parts"])
    lanes = data["lanes"].get("lanes", [])
    lane_html = "".join(
        f"<div class=lane><b>{html.escape(l.get('id', ''))}</b><span class=m>{html.escape(l.get('model', ''))}</span>"
        f"<span>{inline(l.get('task', ''))}</span>"
        f"<span class='st {html.escape(l.get('status', ''))}'>{STATUS.get(l.get('status', ''), l.get('status', ''))}</span></div>"
        for l in lanes) or "<p class=sub>Сейчас агенты не работают.</p>"
    c = data["counts"]
    stats = "".join(f"<span><b>{fmt(v)}</b> {k}</span>" for k, v in [
        ("строк Rust", c["rust_lines"]), ("тестов", c["tests"]), ("крейтов", c["crates"]), ("спецификаций", c["specs"]),
        ("проверок в эмуляторе", c["unicorn_harnesses"]), ("коммитов", c["commits"])])
    log = "".join(f"<li><b>{inline(e['title'])}</b> — {inline(e['text'][:360])}{'…' if len(e['text']) > 360 else ''}</li>"
                  for e in data["log"])
    return f"""<!doctype html><html lang="ru"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>projectx — прогресс</title><meta http-equiv="refresh" content="300"><style>{STYLE}</style></head><body><main>
<h1>projectx — Warcraft III 1.26a на Rust</h1>
<p class=sub>Переписываем Game.dll бит-в-бит · обновлено {html.escape(data['generated'])} · <code>{html.escape(data['commit'])}</code> · <a href="progress.json">progress.json</a></p>
<div class=score>
<div class=card><div class=k>Поведение как в оригинале</div><div class=v>{b['score']:.1f}%</div>{bar(b['score'])}
<div class=note>Главная шкала: насколько игра уже ведёт себя 1:1</div></div>
<div class="card sec"><div class=k>Системы разобраны</div><div class=v>{data['overall']:.1f}%</div>{bar(data['overall'])}
<div class=note>Сколько механизмов найдено и подключено</div></div>
</div>
<h2>Из чего складывается поведение</h2><div class=rows>{brows}</div>
<h2>Системы по частям</h2><div class="rows parts">{prows}</div>
<h2>Системы — динамика</h2>{chart(data['history'])}
<h2>Агенты</h2><div class=lanes>{lane_html}</div>
<h2>Репозиторий</h2><div class=stats>{stats}</div>
<details><summary>Журнал — последние записи</summary><ul class=log>{log}</ul></details>
</main></body></html>"""


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default="site")
    out = Path(ap.parse_args().out)
    text = AGENTS.read_text(encoding="utf-8")
    p = parts(text)
    data = {
        "generated": datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M UTC"),
        "commit": git("rev-parse", "--short", "HEAD").strip(),
        "overall": round(sum(r["weight"] * r["done"] / 100 for r in p), 1),
        "parts": p,
        "history": history(),
        "lanes": json.loads(LANES.read_text(encoding="utf-8")) if LANES.exists() else {},
        "counts": counts(),
        "log": log_entries(text, 12),
        "behaviour": behaviour(),
    }
    out.mkdir(parents=True, exist_ok=True)
    (out / "progress.json").write_text(json.dumps(data, indent=1, ensure_ascii=False), encoding="utf-8")
    (out / "index.html").write_text(page(data), encoding="utf-8")
    print(f"behaviour {data['behaviour']['score']}%, systems {data['overall']}%, {len(p)} parts, {len(data['history'])} history points -> {out}")


if __name__ == "__main__":
    main()
