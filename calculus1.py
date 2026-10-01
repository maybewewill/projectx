"""
Calculus I за одно видео — весь первый семестр (Manim Community, стиль 3Blue1Brown, русский).

Рендер:            manim -qh calculus1.py FullVideo
Одна глава:        manim -qm calculus1.py C17_LHopital
Одна часть:        manim -qh calculus1.py Part3_Derivative

Формат «под конспект»:
  жёлтая рамка «ЗАПИШИ»  — определения, теоремы, формулы для тетради;
  «Пример»               — пошаговый разбор;
  розовая рамка «РЕШИ САМ» — пауза, решаешь, сверяешь ответ.

──────────────────────────────────────────────────────────────────────────────
ПЛАН
──────────────────────────────────────────────────────────────────────────────
Часть I. Функции
  1  Функция, D(f), E(f), чётность
  2  Преобразования графиков, композиция, обратные функции, arc-функции
Часть II. Пределы и непрерывность
  3  Определение предела (ε–δ), односторонние пределы
  4  Вычисление пределов, неопределённость 0/0
  5  Пределы на бесконечности, асимптоты
  6  Теорема о двух милиционерах, 1-й замечательный предел
  7  Число e, 2-й замечательный предел, 1^∞
  8  Эквивалентные бесконечно малые, o-малое
  9  Непрерывность, разрывы, теоремы Больцано–Коши и Вейерштрасса
Часть III. Производная
 10  Определение, касательная
 11  Правила и таблица производных
 12  Цепное правило
 13  Неявная, логарифмическая, обратная, параметрическая
 14  Высшие производные, дифференциал, приближения, метод Ньютона
 15  Связанные скорости
Часть IV. Приложения производной
 16  Теоремы Ферма, Ролля, Лагранжа, Коши
 17  Правило Лопиталя
 18  Монотонность и экстремумы
 19  Выпуклость и перегиб
 20  Полное исследование функции
 21  Оптимизация
 22  Формула Тейлора
Часть V. Интеграл
 23  Первообразная и таблица
 24  Замена переменной и интегрирование по частям
 25  Определённый интеграл, ОТА, Ньютон–Лейбниц
 26  Площадь между кривыми, среднее значение
 27  Итог
"""

from manim import *
import numpy as np

# ───────────────────────────── стиль ──────────────────────────────────────
C_BG = "#0F1115"
C_PANEL = "#161B26"
C_F = BLUE_C
C_F2 = TEAL_C
C_POS = BLUE_D
C_NEG = RED_D
C_A = YELLOW
C_H = GREEN_C
C_DY = RED_C
C_LIM = TEAL_B
C_BAD = RED
C_GOOD = GREEN_B
C_PINK = "#FF79B0"

config.background_color = C_BG

RU_TEX = TexTemplate(
    preamble=r"""
\usepackage[T2A]{fontenc}
\usepackage[utf8]{inputenc}
\usepackage[russian]{babel}
\usepackage{amsmath}
\usepackage{amssymb}
"""
)
config.tex_template = RU_TEX

FONT = "CMU Serif"

# темп: быстрый читатель
READ_BASE, READ_PER_CHAR = 0.35, 0.04


def T(text, size=34, color=WHITE, **kw):
    return Text(text, font=FONT, font_size=size, color=color, **kw)


def M(*tex, size=40, color=WHITE, **kw):
    return MathTex(*tex, font_size=size, color=color, **kw)


def ru(v, d=3):
    return f"{v:.{d}f}".replace(".", ",").replace("-", "−")


def wrap(text, width=70):
    words, lines, cur = text.split(), [], ""
    for w in words:
        if cur and len(cur) + 1 + len(w) > width:
            lines.append(cur)
            cur = w
        else:
            cur = f"{cur} {w}".strip()
    if cur:
        lines.append(cur)
    return lines


def place(m, x0=-6.9, x1=6.9, y0=-2.62, y1=3.45):
    """Вписать объект в рабочую область (над субтитрами, под заголовком главы)."""
    if m.width > x1 - x0:
        m.scale_to_fit_width(x1 - x0)
    if m.height > y1 - y0:
        m.scale_to_fit_height(y1 - y0)
    dx = max(0, x0 - m.get_left()[0]) - max(0, m.get_right()[0] - x1)
    dy = max(0, y0 - m.get_bottom()[1]) - max(0, m.get_top()[1] - y1)
    m.shift(dx * RIGHT + dy * UP)
    return m


def live(getter, place_fn, fmt="{}", d=3, size=32, color=WHITE):
    return always_redraw(lambda: place_fn(T(fmt.format(ru(getter(), d)), size, color)))


def std_axes(xr, yr, xl, yl, nums=True, font=20):
    return Axes(x_range=xr, y_range=yr, x_length=xl, y_length=yl, tips=False,
                axis_config={"include_numbers": nums, "font_size": font, "color": GREY_B,
                             "stroke_width": 2})


def curve(ax, f, x0, x1, color=C_F, width=4, n=500, ylim=None):
    """График с автоматическим обрезанием по оси y и разрывами (асимптоты, дыры)."""
    lo, hi = ylim if ylim else (ax.y_range[0], ax.y_range[1])
    segs, cur = VGroup(), []
    with np.errstate(all="ignore"):
        for x in np.linspace(x0, x1, n):
            try:
                y = float(f(x))
            except (ZeroDivisionError, ValueError, OverflowError):
                y = np.nan
            if np.isfinite(y) and lo <= y <= hi:
                cur.append(ax.c2p(x, y))
            else:
                if len(cur) > 1:
                    segs.add(VMobject(color=color, stroke_width=width).set_points_as_corners(cur))
                cur = []
    if len(cur) > 1:
        segs.add(VMobject(color=color, stroke_width=width).set_points_as_corners(cur))
    return segs


def area_between(ax, f, g, a, b, color=C_POS, opacity=0.45, n=160):
    xs = np.linspace(a, b, n)
    pts = [ax.c2p(x, f(x)) for x in xs] + [ax.c2p(x, g(x)) for x in xs[::-1]]
    return Polygon(*pts, stroke_width=0, fill_color=color, fill_opacity=opacity)


def riemann(ax, f, a, b, n, color=C_POS, opacity=0.6):
    dx = (b - a) / n
    g = VGroup()
    for k in range(n):
        x0 = a + k * dx
        y = f(x0 + dx)
        p0, p1 = ax.c2p(x0, 0), ax.c2p(x0 + dx, y)
        r = Rectangle(width=abs(p1[0] - p0[0]), height=max(abs(p1[1] - p0[1]), 1e-3))
        r.set_fill(color, opacity).set_stroke(WHITE, 1 if n <= 40 else 0.3)
        g.add(r.move_to((p0 + p1) / 2))
    return g


def tangent(ax, f, fp, x, length=3.0, color=C_A, width=3):
    p = ax.c2p(x, f(x))
    d = ax.c2p(x + 1, f(x) + fp(x)) - p
    d = d / np.linalg.norm(d)
    return Line(p - d * length / 2, p + d * length / 2, color=color, stroke_width=width)


def hole(point, color=C_F, r=0.07):
    return Circle(radius=r, color=color, stroke_width=3).set_fill(C_BG, 1).move_to(point)


# ═════════════════════════ базовая сцена ══════════════════════════════════
class Story(Scene):
    def setup(self):
        self.cap = None
        self.hdr = None

    # --- субтитры
    def say(self, text, wait=None, size=26):
        lines = wrap(text)
        new = VGroup(*[T(l, size, GREY_A) for l in lines]).arrange(DOWN, buff=0.1)
        if new.width > 13.5:
            new.scale_to_fit_width(13.5)
        new.to_edge(DOWN, buff=0.25)
        anims = [FadeIn(new, shift=0.1 * UP)]
        if self.cap is not None:
            anims.append(FadeOut(self.cap, shift=0.1 * UP))
        self.play(*anims, run_time=0.35)
        self.cap = new
        self.wait(wait if wait is not None else READ_BASE + READ_PER_CHAR * len(text))

    def clear_all(self, keep_hdr=True):
        mobs = [m for m in self.mobjects if not (keep_hdr and m is self.hdr)]
        for m in mobs:
            m.clear_updaters()
        if mobs:
            self.play(*[FadeOut(m) for m in mobs], run_time=0.45)
        self.cap = None
        if not keep_hdr:
            self.hdr = None

    # --- заставки
    def part(self, roman, title):
        self.clear_all(keep_hdr=False)
        a = T(f"Часть {roman}", 34, GREY_B)
        b = T(title, 62, C_A)
        g = place(VGroup(a, b).arrange(DOWN, buff=0.3))
        self.play(FadeIn(a), Write(b), run_time=1.0)
        self.wait(0.8)
        self.play(FadeOut(g), run_time=0.4)

    def chapter(self, num, title):
        self.clear_all(keep_hdr=False)
        n = T(f"{num}", 44, C_A)
        t = T(title, 46)
        g = place(VGroup(n, t).arrange(RIGHT, buff=0.4))
        self.play(FadeIn(n), Write(t), run_time=0.8)
        self.wait(0.5)
        self.hdr = T(f"{num}. {title}", 20, GREY_B).to_corner(UL, buff=0.22)
        self.play(ReplacementTransform(g, self.hdr), run_time=0.5)

    # --- блоки конспекта
    def note(self, *lines, title="Запиши", pos=ORIGIN, size=34, max_w=13.6, max_h=6.0, wait=1.6):
        body = VGroup(*[T(l[2:], 26) if l.startswith("T:") else M(l, size=size) for l in lines])
        body.arrange(DOWN, buff=0.22, aligned_edge=LEFT)
        hd = T(title.upper(), 22, C_A)
        g = VGroup(hd, body).arrange(DOWN, buff=0.25, aligned_edge=LEFT)
        if g.width > max_w - 0.5:
            g.scale_to_fit_width(max_w - 0.5)
        if g.height > max_h - 0.5:
            g.scale_to_fit_height(max_h - 0.5)
        box = SurroundingRectangle(g, buff=0.22, corner_radius=0.12, color=C_A, stroke_width=2)
        box.set_fill(C_PANEL, 1)
        panel = place(VGroup(box, g).move_to(pos))
        self.play(FadeIn(box), FadeIn(hd),
                  LaggedStart(*[FadeIn(x, shift=0.1 * RIGHT) for x in body], lag_ratio=0.25),
                  run_time=0.5 + 0.25 * len(lines))
        self.wait(wait)
        return panel

    def derive(self, lhs, steps, size=34, buff=0.26, pos=ORIGIN, max_w=13.4, max_h=5.6, rt=0.8, eq="="):
        rows = [M(lhs, eq, steps[0][0], size=size)]
        for tex, _ in steps[1:]:
            rows.append(M(eq, tex, size=size))
        g = VGroup(*rows).arrange(DOWN, buff=buff)
        for r in rows[1:]:
            r.shift((rows[0][1].get_left()[0] - r[0].get_left()[0]) * RIGHT)
        if g.height > max_h:
            g.scale_to_fit_height(max_h)
        if g.width > max_w:
            g.scale_to_fit_width(max_w)
        place(g.move_to(pos))
        for i, (row, (_, cap)) in enumerate(zip(rows, steps)):
            if i == 0:
                self.play(Write(row), run_time=rt + 0.3)
            else:
                self.play(TransformMatchingShapes(rows[i - 1][-1].copy(), row), run_time=rt)
            if cap:
                self.say(cap)
            else:
                self.wait(0.5)
        return g

    def example(self, lhs, steps, label="Пример", size=34, clear=True, **kw):
        lab = T(label, 28, C_A).to_edge(UP, buff=0.55)
        self.play(FadeIn(lab, shift=0.1 * DOWN), run_time=0.3)
        kw.setdefault("pos", DOWN * 0.15)
        kw.setdefault("max_h", 5.2)
        g = self.derive(lhs, steps, size=size, **kw)
        self.wait(1.0)
        if clear:
            self.clear_all()
        return g

    def practice(self, problem, answer, solution=(), size=40):
        lbl = T("РЕШИ САМ", 30, C_PINK)
        sub = T("пауза → решаешь → сверяешь", 22, GREY_B)
        prob = M(problem, size=size)
        g = VGroup(lbl, sub, prob).arrange(DOWN, buff=0.25).move_to(UP * 1.7)
        box = SurroundingRectangle(g, color=C_PINK, buff=0.25, corner_radius=0.12)
        grp = place(VGroup(box, g))
        self.play(Create(box), FadeIn(lbl), FadeIn(sub), Write(prob), run_time=0.9)
        self.wait(2.2)
        items = [M(s, size=size - 6, color=GREY_A) for s in solution]
        items.append(M(r"\text{Ответ: }" + answer, size=size, color=C_GOOD))
        sol = VGroup(*items).arrange(DOWN, buff=0.22).next_to(grp, DOWN, buff=0.35)
        place(sol, y1=grp.get_bottom()[1] - 0.15)
        for it in items:
            self.play(Write(it), run_time=0.7)
        self.wait(1.6)
        self.clear_all()

    # ═════════════════════════════ 0. Вступление ═════════════════════════
    def c00_intro(self):
        t = T("Calculus I", 84, C_A)
        s = T("весь первый семестр в одном видео", 34, GREY_A)
        VGroup(t, s).arrange(DOWN, buff=0.4)
        self.play(Write(t), FadeIn(s, shift=0.2 * UP), run_time=1.4)
        self.wait(1.0)
        self.play(FadeOut(t), FadeOut(s), run_time=0.4)
        parts = [("I", "Функции", "область определения, чётность, преобразования, обратные"),
                 ("II", "Пределы и непрерывность", "ε–δ, неопределённости, замечательные пределы, эквивалентности, разрывы"),
                 ("III", "Производная", "касательная, правила, таблица, цепное правило, неявные функции, приближения"),
                 ("IV", "Приложения производной", "теоремы о среднем, Лопиталь, экстремумы, графики, оптимизация, Тейлор"),
                 ("V", "Интеграл", "первообразные, замена, по частям, Ньютон–Лейбниц, площади")]
        rows = VGroup(*[VGroup(T(p, 34, C_A), VGroup(T(n, 32), T(d, 20, GREY_B))
                               .arrange(DOWN, aligned_edge=LEFT, buff=0.08)).arrange(RIGHT, buff=0.4)
                        for p, n, d in parts]).arrange(DOWN, aligned_edge=LEFT, buff=0.3)
        place(rows.move_to(UP * 0.4))
        self.play(LaggedStart(*[FadeIn(r, shift=RIGHT * 0.3) for r in rows], lag_ratio=0.2), run_time=1.8)
        self.say("Пять частей, 27 глав. В каждой теме: идея с картинкой, конспект, разбор примеров "
                 "и задача для самопроверки.")
        self.clear_all()
        legend = VGroup(
            VGroup(RoundedRectangle(corner_radius=0.08, width=0.7, height=0.45, color=C_A),
                   T("«Запиши» — переписывай в тетрадь", 30)).arrange(RIGHT, buff=0.35),
            VGroup(Rectangle(width=0.7, height=0.45, color=C_A).set_opacity(0),
                   T("«Пример» — разбор по шагам", 30)).arrange(RIGHT, buff=0.35),
            VGroup(RoundedRectangle(corner_radius=0.08, width=0.7, height=0.45, color=C_PINK),
                   T("«Реши сам» — пауза, решаешь, сверяешь ответ", 30)).arrange(RIGHT, buff=0.35),
        ).arrange(DOWN, aligned_edge=LEFT, buff=0.4)
        legend[1][0].set_stroke(opacity=0)
        legend[1][1].set_color(C_A)
        self.play(FadeIn(legend))
        self.say("Темп быстрый: если не успеваешь записать — ставь на паузу. Поехали.")

    # ═════════════════════════════ ЧАСТЬ I ═══════════════════════════════
    def c01_functions(self):
        self.chapter(1, "Функция и её свойства")
        ax = place(std_axes([-3, 3, 1], [-1, 5, 1], 5.4, 4.2).move_to(LEFT * 3.6 + UP * 0.4))
        g = ax.plot(lambda x: x ** 2, x_range=[-2.2, 2.2], color=C_F, stroke_width=4)
        self.play(Create(ax), Create(g), run_time=0.8)
        x = ValueTracker(-1.8)
        dot = always_redraw(lambda: Dot(ax.c2p(x.get_value(), x.get_value() ** 2), color=C_A))
        lns = always_redraw(lambda: ax.get_lines_to_point(ax.c2p(x.get_value(), x.get_value() ** 2), color=GREY_B))
        self.add(lns, dot)
        self.play(x.animate.set_value(1.6), run_time=1.6)
        self.note(r"f:\ D(f)\to E(f),\qquad x\mapsto y=f(x)",
                  r"\text{Запреты при поиске } D(f):",
                  r"\frac{1}{g}:\ g\ne 0\qquad \sqrt[2k]{g}:\ g\ge 0",
                  r"\log_a g:\ g>0\qquad \arcsin g,\ \arccos g:\ |g|\le 1",
                  pos=RIGHT * 3.3 + UP * 0.4, size=30, max_w=7.2)
        self.say("Функция — правило: каждому x из области определения D(f) ровно одно y. "
                 "E(f) — множество значений. D(f) находим, выписывая все запреты.")
        self.clear_all()

        a1 = place(std_axes([-2, 2, 1], [-1, 4, 1], 4.8, 3.4, nums=False).move_to(LEFT * 3.4 + UP * 1.0))
        a2 = place(std_axes([-2, 2, 1], [-3, 3, 1], 4.8, 3.4, nums=False).move_to(RIGHT * 3.4 + UP * 1.0))
        g1 = a1.plot(lambda x: x ** 2, x_range=[-1.9, 1.9], color=C_F, stroke_width=4)
        g2 = a2.plot(lambda x: 0.4 * x ** 3, x_range=[-1.9, 1.9], color=C_F2, stroke_width=4)
        self.play(Create(a1), Create(a2), Create(g1), Create(g2), run_time=0.9)
        d1 = VGroup(Dot(a1.c2p(-1.3, 1.69), color=C_A), Dot(a1.c2p(1.3, 1.69), color=C_A),
                    DashedLine(a1.c2p(-1.3, 1.69), a1.c2p(1.3, 1.69), color=C_A))
        d2 = VGroup(Dot(a2.c2p(-1.5, -1.35), color=C_A), Dot(a2.c2p(1.5, 1.35), color=C_A),
                    DashedLine(a2.c2p(-1.5, -1.35), a2.c2p(1.5, 1.35), color=C_A))
        t1 = M(r"\text{чётная: } f(-x)=f(x)", size=30).next_to(a1, DOWN, buff=0.2)
        t2 = M(r"\text{нечётная: } f(-x)=-f(x)", size=30).next_to(a2, DOWN, buff=0.2)
        self.play(FadeIn(d1), FadeIn(d2), Write(t1), Write(t2), run_time=0.9)
        self.say("Чётная — симметрия относительно Oy (x², cos x, |x|). Нечётная — относительно "
                 "начала координат (x³, sin x, tg x). D(f) при этом обязана быть симметричной.")
        self.say("Ещё свойства: периодичность f(x+T) = f(x) (sin, cos — период 2π, tg — π), "
                 "монотонность, ограниченность.")
        self.clear_all()
        self.example(r"D\!\left(\frac{\sqrt{4-x^2}}{x-1}\right)", [
            (r"\left\{\begin{aligned}&4-x^2\ge 0\\ &x-1\ne 0\end{aligned}\right.", "Корень: подкоренное ≥ 0. Знаменатель ≠ 0."),
            (r"\left\{\begin{aligned}&-2\le x\le 2\\ &x\ne 1\end{aligned}\right.", None),
            (r"[-2;\,1)\cup(1;\,2]", None)])
        self.practice(r"D\big(\ln(x^2-9)\big)=\ ?", r"(-\infty;-3)\cup(3;+\infty)", [r"x^2-9>0\iff |x|>3"])

    def c02_transforms(self):
        self.chapter(2, "Преобразования графиков и обратные функции")
        ax = place(std_axes([-4, 4, 1], [-3, 5, 1], 6.4, 4.8).move_to(LEFT * 3.1 + UP * 0.4))
        base = ax.plot(lambda x: x ** 2, x_range=[-2.2, 2.2], color=GREY_B, stroke_width=2, stroke_opacity=0.5)
        a, b, k = ValueTracker(0), ValueTracker(0), ValueTracker(1)
        gr = always_redraw(lambda: curve(ax, lambda x: k.get_value() * (x - a.get_value()) ** 2 + b.get_value(), -4, 4, n=240))
        sg = lambda v: (" − " if v < 0 else " + ") + ru(abs(v), 1)
        lab = always_redraw(lambda: T(f"y = {ru(k.get_value(), 1)}·(x{sg(-a.get_value())})²{sg(b.get_value())}",
                                      30, C_F).move_to(RIGHT * 3.4 + UP * 2.8))
        self.play(Create(ax), FadeIn(base), FadeIn(gr), FadeIn(lab), run_time=0.8)
        self.play(a.animate.set_value(2), run_time=1.2)
        self.play(b.animate.set_value(1.5), run_time=1.0)
        self.play(k.animate.set_value(-0.5), run_time=1.3)
        self.play(a.animate.set_value(-1), b.animate.set_value(3), k.animate.set_value(1), run_time=1.3)
        self.note(r"f(x-a):\ \text{сдвиг вправо на } a",
                  r"f(x)+b:\ \text{сдвиг вверх на } b",
                  r"k\,f(x):\ \text{растяжение по } Oy \text{ в } k \text{ раз}",
                  r"f(kx):\ \text{сжатие по } Ox \text{ в } k \text{ раз}",
                  r"-f(x):\ \text{отражение отн. } Ox",
                  r"f(-x):\ \text{отражение отн. } Oy",
                  pos=RIGHT * 3.4 + DOWN * 0.3, size=28, max_w=7.0)
        self.say("Внимание: сдвиг «внутри» идёт в обратную сторону: f(x − 2) — это сдвиг вправо.")
        self.clear_all()

        ax = place(std_axes([-3, 4, 1], [-3, 4, 1], 5.0, 5.0).move_to(LEFT * 3.6 + UP * 0.4))
        e = curve(ax, np.exp, -3, 4, color=C_F)
        diag = DashedLine(ax.c2p(-3, -3), ax.c2p(4, 4), color=GREY_B)
        ln = curve(ax, np.log, 0.01, 4, color=C_A)
        self.play(Create(ax), Create(e), Create(diag), run_time=0.9)
        self.play(TransformFromCopy(e, ln), run_time=1.2)
        self.note(r"(f\circ g)(x)=f\big(g(x)\big)",
                  r"y=f(x)\iff x=f^{-1}(y)",
                  r"\text{график } f^{-1} \text{ — отражение отн. } y=x",
                  r"\text{существует} \iff f \text{ строго монотонна}",
                  r"\arcsin:[-1;1]\to[-\tfrac{\pi}{2};\tfrac{\pi}{2}]",
                  r"\arccos:[-1;1]\to[0;\pi]",
                  r"\operatorname{arctg}:\mathbb{R}\to(-\tfrac{\pi}{2};\tfrac{\pi}{2})",
                  pos=RIGHT * 3.3 + UP * 0.4, size=28, max_w=7.2)
        self.say("Обратная функция «отменяет» f: ln отменяет eˣ, arcsin — синус на [−π/2; π/2]. "
                 "Чтобы найти f⁻¹: выразить x через y и поменять буквы местами.")
        self.clear_all()
        self.practice(r"y=\frac{2x+1}{x-3}.\quad f^{-1}(x)=\ ?", r"f^{-1}(x)=\frac{3x+1}{x-2}",
                      [r"y(x-3)=2x+1\ \Rightarrow\ x(y-2)=3y+1"])

    # ═════════════════════════════ ЧАСТЬ II ══════════════════════════════
    def c03_limit_def(self):
        self.chapter(3, "Предел: определение")
        ax = place(std_axes([-1, 3, 1], [-1, 4, 1], 5.6, 4.4).move_to(LEFT * 3.4 + UP * 0.35))
        g = curve(ax, lambda x: x + 1, -1, 3)
        hh = hole(ax.c2p(1, 2))
        self.play(Create(ax), Create(g), FadeIn(hh), run_time=0.8)
        eps = ValueTracker(1.0)
        eb = always_redraw(lambda: Rectangle(width=ax.x_length, height=abs(ax.c2p(0, 2 + eps.get_value())[1] - ax.c2p(0, 2 - eps.get_value())[1]))
                           .set_stroke(width=0).set_fill(C_PINK, 0.18).move_to(ax.c2p(1, 2)).set_x(ax.get_center()[0]))
        db = always_redraw(lambda: Rectangle(height=ax.y_length, width=abs(ax.c2p(1 + eps.get_value(), 0)[0] - ax.c2p(1 - eps.get_value(), 0)[0]))
                           .set_stroke(width=0).set_fill(C_H, 0.18).move_to(ax.c2p(1, 0)).set_y(ax.get_center()[1]))
        self.add(eb, db)
        ev = live(lambda: eps.get_value(), lambda m: m.move_to(RIGHT * 3.4 + UP * 2.9), "ε = {}", d=2, color=C_PINK)
        dv = live(lambda: eps.get_value(), lambda m: m.move_to(RIGHT * 3.4 + UP * 2.3), "δ = {}", d=2, color=C_H)
        self.play(FadeIn(eb), FadeIn(db), FadeIn(ev), FadeIn(dv), run_time=0.6)
        self.say("f(x) = (x² − 1)/(x − 1): в точке 1 дырка, но рядом значения близки к 2. "
                 "Для любого коридора ε вокруг 2 есть коридор δ вокруг 1, где график внутри.")
        self.play(eps.animate.set_value(0.25), run_time=2)
        self.note(r"\lim_{x\to a}f(x)=L\iff",
                  r"\forall\varepsilon>0\ \exists\delta>0:\ 0<|x-a|<\delta\Rightarrow|f(x)-L|<\varepsilon",
                  r"\lim_{x\to a}f=L\iff \lim_{x\to a-0}f=\lim_{x\to a+0}f=L",
                  r"\lim_{x\to+\infty}f=L:\ \forall\varepsilon\ \exists N:\ x>N\Rightarrow|f(x)-L|<\varepsilon",
                  pos=RIGHT * 2.4 + DOWN * 0.6, size=28, max_w=9.0)
        self.say("Значение f(a) роли не играет: 0 < |x − a|. Предел существует, только если левый и правый "
                 "пределы существуют и равны.")
        self.clear_all()
        self.example(r"\lim_{x\to 2}(3x-1)=5:\quad |f(x)-5|", [
            (r"|3x-6|=3|x-2|", "Оцениваем расстояние до предела через |x − a|."),
            (r"3|x-2|<3\delta", None),
            (r"\varepsilon\quad\text{при}\quad \delta=\varepsilon/3", "Выбираем δ = ε/3 — для любого ε. Предел доказан.")],
            label="Пример: доказать по определению")
        self.practice(r"\lim_{x\to 0}\frac{x}{|x|}=\ ?", r"\text{не существует}",
                      [r"\lim_{x\to0-}=-1,\qquad \lim_{x\to0+}=1"])

    def c04_limit_calc(self):
        self.chapter(4, "Вычисление пределов, 0/0")
        self.note(r"\lim(f\pm g)=\lim f\pm\lim g,\quad \lim(fg)=\lim f\cdot\lim g",
                  r"\lim\frac fg=\frac{\lim f}{\lim g}\quad(\lim g\ne 0)",
                  r"\text{непрерывная функция: просто подставь } x=a",
                  r"\text{неопределённости: }\ \tfrac00,\ \tfrac{\infty}{\infty},\ \infty-\infty,\ 0\cdot\infty,\ 1^{\infty},\ 0^0,\ \infty^0",
                  pos=UP * 0.6, size=32)
        self.say("Сначала всегда подставляем. Если получилось число — это ответ. Если неопределённость — "
                 "преобразуем выражение.")
        self.clear_all()
        ax = place(std_axes([-1, 4, 1], [-1, 6, 1], 5.0, 4.2).move_to(LEFT * 4.0 + UP * 0.3))
        g = curve(ax, lambda x: x + 2, -1, 4)
        self.play(Create(ax), Create(g), FadeIn(hole(ax.c2p(2, 4))), run_time=0.8)
        self.derive(r"\lim_{x\to2}\frac{x^2-4}{x-2}", [
            (r"\left[\tfrac00\right]", "Подстановка даёт 0/0 — надо преобразовать."),
            (r"\lim_{x\to2}\frac{(x-2)(x+2)}{x-2}", "Раскладываем на множители: общий множитель (x − 2)."),
            (r"\lim_{x\to2}(x+2)=4", "Сокращаем (x ≠ 2 под пределом) и подставляем.")],
            pos=RIGHT * 2.6 + UP * 0.3, max_w=8.2)
        self.clear_all()
        self.example(r"\lim_{x\to0}\frac{\sqrt{x+1}-1}{x}", [
            (r"\lim_{x\to0}\frac{(\sqrt{x+1}-1)(\sqrt{x+1}+1)}{x(\sqrt{x+1}+1)}", "Корни → домножаем на сопряжённое."),
            (r"\lim_{x\to0}\frac{x}{x(\sqrt{x+1}+1)}", "(a − b)(a + b) = a² − b²."),
            (r"\lim_{x\to0}\frac{1}{\sqrt{x+1}+1}=\frac12", None)])
        self.practice(r"\lim_{x\to3}\frac{x^2-9}{x^2-5x+6}=\ ?", r"6",
                      [r"\frac{(x-3)(x+3)}{(x-3)(x-2)}=\frac{x+3}{x-2}\to\frac{6}{1}"])

    def c05_infinity(self):
        self.chapter(5, "Пределы на бесконечности, асимптоты")
        self.note(r"\lim_{x\to\infty}\frac{1}{x^p}=0\quad(p>0)",
                  r"\frac{P_n(x)}{Q_m(x)}\to\begin{cases}\frac{a_n}{b_m},& n=m\\ 0,& n<m\\ \infty,& n>m\end{cases}",
                  r"\text{вертикальная: } x=a,\ \text{если } \lim_{x\to a}f=\infty",
                  r"\text{горизонтальная: } y=L=\lim_{x\to\pm\infty}f",
                  r"\text{наклонная } y=kx+b:\ k=\lim\frac{f(x)}{x},\ b=\lim\big(f(x)-kx\big)",
                  pos=RIGHT * 3.45, size=28, max_w=6.9)
        ax = place(std_axes([-4, 5, 1], [-8, 10, 2], 5.4, 5.4).move_to(LEFT * 3.9 + UP * 0.3))
        f = lambda x: (x ** 2 + 1) / (x - 1)
        g = curve(ax, f, -4, 5, n=800)
        va = DashedLine(ax.c2p(1, -8), ax.c2p(1, 10), color=C_PINK)
        oa = DashedLine(ax.c2p(-4, -3), ax.c2p(5, 6), color=C_A)
        lbl = M(r"y=\frac{x^2+1}{x-1}", size=28, color=C_F).move_to(ax.c2p(3.4, 8.6))
        self.play(Create(ax), Create(g), Write(lbl), run_time=1)
        self.play(Create(va), Create(oa), run_time=0.8)
        self.say("Пример: вертикальная асимптота x = 1 (знаменатель → 0), наклонная y = x + 1, "
                 "потому что (x² + 1)/(x − 1) = x + 1 + 2/(x − 1).")
        self.clear_all()
        self.example(r"\lim_{x\to\infty}\frac{3x^2+x}{2x^2-5}", [
            (r"\lim_{x\to\infty}\frac{3+\frac1x}{2-\frac{5}{x^2}}", "Делим числитель и знаменатель на старшую степень x²."),
            (r"\frac{3+0}{2-0}=\frac32", None)])
        self.example(r"\lim_{x\to+\infty}\big(\sqrt{x^2+x}-x\big)", [
            (r"\lim\frac{(\sqrt{x^2+x}-x)(\sqrt{x^2+x}+x)}{\sqrt{x^2+x}+x}", "∞ − ∞ с корнем → сопряжённое."),
            (r"\lim\frac{x}{\sqrt{x^2+x}+x}", None),
            (r"\lim\frac{1}{\sqrt{1+\frac1x}+1}=\frac12", "Делим на x.")])
        self.practice(r"\text{Асимптоты } y=\frac{2x^2}{x+1}", r"x=-1,\quad y=2x-2",
                      [r"k=\lim\frac{2x^2}{x(x+1)}=2,\quad b=\lim\left(\frac{2x^2}{x+1}-2x\right)=\lim\frac{-2x}{x+1}=-2"])

    def c06_squeeze(self):
        self.chapter(6, "Два милиционера и 1-й замечательный предел")
        ax = place(std_axes([-1, 1, 0.5], [-1, 1, 0.5], 5.6, 4.0, nums=False).move_to(LEFT * 3.6 + UP * 0.6))
        up = curve(ax, lambda x: x ** 2, -1, 1, color=GREY_B, width=2)
        dn = curve(ax, lambda x: -x ** 2, -1, 1, color=GREY_B, width=2)
        mid = curve(ax, lambda x: x ** 2 * np.sin(1 / x), -1, 1, color=C_F, width=3, n=3000)
        self.play(Create(ax), Create(up), Create(dn), run_time=0.7)
        self.play(Create(mid), run_time=1.4)
        self.note(r"g(x)\le f(x)\le h(x),\ \ \lim g=\lim h=L",
                  r"\Rightarrow\ \lim f=L",
                  pos=RIGHT * 3.4 + UP * 1.6, size=30, max_w=7)
        self.say("x² sin(1/x) зажата между −x² и x², обе → 0. Значит и она → 0 — как подозреваемый "
                 "между двумя милиционерами.")
        self.clear_all()
        O = LEFT * 4.2 + DOWN * 1.9
        R, x = 3.6, 0.75
        A = O + R * RIGHT
        P = O + R * np.array([np.cos(x), np.sin(x), 0])
        Tp = O + R * np.array([1, np.tan(x), 0])
        arc = Arc(radius=R, start_angle=0, angle=PI / 2, arc_center=O, color=GREY_B)
        tri1 = Polygon(O, A, P, stroke_width=2, color=C_F).set_fill(C_F, 0.35)
        sector = Polygon(O, *[O + R * np.array([np.cos(t), np.sin(t), 0]) for t in np.linspace(0, x, 40)],
                         stroke_width=0).set_fill(C_A, 0.35)
        tri2 = Polygon(O, A, Tp, stroke_width=2, color=C_PINK).set_fill(C_PINK, 0.2)
        sinl = Line(P, np.array([P[0], O[1], 0]), color=C_F, stroke_width=4)
        tanl = Line(A, Tp, color=C_PINK, stroke_width=4)
        ls = M(r"\sin x", size=30, color=C_F).next_to(sinl, LEFT, buff=0.08)
        lt = M(r"\operatorname{tg} x", size=30, color=C_PINK).next_to(tanl, RIGHT, buff=0.1)
        lx = M("x", size=30, color=C_A).move_to(O + 0.8 * np.array([np.cos(x / 2), np.sin(x / 2), 0]))
        self.play(Create(arc), Create(Line(O, A, color=GREY_B)), run_time=0.6)
        self.play(FadeIn(tri1), Create(sinl), Write(ls), run_time=0.6)
        self.play(FadeIn(sector), Write(lx), run_time=0.6)
        self.play(FadeIn(tri2), Create(tanl), Write(lt), run_time=0.6)
        ineq = VGroup(
            M(r"S_{\triangle}<S_{\text{сект}}<S_{\triangle}", size=32),
            M(r"\tfrac12\sin x<\tfrac12x<\tfrac12\operatorname{tg}x", size=32),
            M(r"\cos x<\frac{\sin x}{x}<1", size=34),
            M(r"\lim_{x\to0}\frac{\sin x}{x}=1", size=40, color=C_A),
        ).arrange(DOWN, buff=0.3).move_to(RIGHT * 3.6 + UP * 0.8)
        for m in ineq:
            self.play(Write(m), run_time=0.7)
        self.say("Площади: треугольник < сектор < большой треугольник. Делим на ½ sin x и переворачиваем. "
                 "cos x → 1, значит по милиционерам sin x / x → 1.")
        self.clear_all()
        self.note(r"\lim_{x\to0}\frac{\sin x}{x}=1",
                  r"\lim_{x\to0}\frac{\operatorname{tg}x}{x}=1,\quad \lim_{x\to0}\frac{\arcsin x}{x}=1",
                  r"\lim_{x\to0}\frac{1-\cos x}{x^2}=\frac12",
                  pos=UP * 0.8)
        self.wait(0.5)
        self.clear_all()
        self.example(r"\lim_{x\to0}\frac{1-\cos x}{x^2}", [
            (r"\lim\frac{2\sin^2\frac x2}{x^2}", "1 − cos x = 2 sin²(x/2)."),
            (r"\lim\frac{2}{4}\cdot\left(\frac{\sin\frac x2}{\frac x2}\right)^2=\frac12", "Подгоняем под замечательный предел.")])
        self.practice(r"\lim_{x\to0}\frac{\operatorname{tg}3x}{\sin 2x}=\ ?", r"\frac32",
                      [r"\frac{\operatorname{tg}3x}{3x}\cdot\frac{2x}{\sin2x}\cdot\frac{3x}{2x}\to1\cdot1\cdot\frac32"])

    def c07_e(self):
        self.chapter(7, "Число e и 2-й замечательный предел")
        ax = place(std_axes([0, 30, 5], [1.8, 2.9, 0.2], 6.4, 4.0, nums=False).move_to(LEFT * 3.3 + UP * 0.6))
        ax.add_coordinates({5 * i: M(str(5 * i), size=20) for i in range(1, 7)},
                           {2: M("2", size=20), 2.5: M("2{,}5", size=20)})
        g = curve(ax, lambda x: (1 + 1 / x) ** x, 1, 30, color=C_F)
        el = DashedLine(ax.c2p(0, np.e), ax.c2p(30, np.e), color=C_A)
        elab = M("e", size=34, color=C_A).next_to(el, RIGHT, buff=0.1)
        self.play(Create(ax), Create(g), Create(el), Write(elab), run_time=1)
        tbl = VGroup(*[M(fr"n={n}:\ \ {v}", size=30) for n, v in
                       [(1, "2"), (12, "2{,}613"), (365, "2{,}7146"), (r"10^6", "2{,}71828")]]).arrange(DOWN, aligned_edge=LEFT, buff=0.22)
        tbl.move_to(RIGHT * 3.8 + UP * 1.4)
        self.play(LaggedStart(*[FadeIn(t) for t in tbl], lag_ratio=0.2), run_time=1)
        self.say("Вклад 1 рубль под 100% годовых, проценты начисляются n раз в год: (1 + 1/n)ⁿ. "
                 "При n → ∞ получаем не бесконечность, а число e ≈ 2,71828.")
        self.clear_all()
        self.note(r"e=\lim_{n\to\infty}\left(1+\frac1n\right)^n=\lim_{x\to0}(1+x)^{1/x}\approx2{,}718",
                  r"\lim_{x\to\infty}\left(1+\frac ax\right)^{x}=e^{a}",
                  r"\text{неопр. } 1^\infty:\quad \lim f^{\,g}=e^{\lim g\,(f-1)}\quad(f\to1,\ g\to\infty)",
                  pos=UP * 0.8, size=34)
        self.clear_all()
        self.example(r"\lim_{x\to\infty}\left(\frac{x+1}{x-1}\right)^{x}", [
            (r"[1^\infty]", "Основание → 1, показатель → ∞."),
            (r"e^{\lim x\left(\frac{x+1}{x-1}-1\right)}", "Используем формулу e^{lim g(f − 1)}."),
            (r"e^{\lim\frac{2x}{x-1}}=e^{2}", None)])
        self.practice(r"\lim_{x\to\infty}\left(1-\frac2x\right)^{3x}=\ ?", r"e^{-6}",
                      [r"e^{\lim 3x\cdot(-2/x)}=e^{-6}"])

    def c08_equiv(self):
        self.chapter(8, "Эквивалентные бесконечно малые")
        ax = place(std_axes([-1, 1, 0.5], [-1, 1.5, 0.5], 5.4, 4.6, nums=False).move_to(LEFT * 4.0 + UP * 0.4))
        fs = [(lambda x: x, WHITE, "x"), (np.sin, C_F, r"\sin x"), (np.tan, C_PINK, r"\operatorname{tg}x"),
              (lambda x: np.log(1 + x), C_A, r"\ln(1+x)"), (lambda x: np.exp(x) - 1, C_H, r"e^x-1")]
        gs = VGroup(*[curve(ax, f, -0.95, 1, color=c, width=3) for f, c, _ in fs])
        legend = VGroup(*[M(t, size=24, color=c) for _, c, t in fs]).arrange(DOWN, aligned_edge=LEFT, buff=0.1)
        legend.next_to(ax, LEFT, buff=0.05).shift(UP * 1.2)
        place(legend)
        self.play(Create(ax), LaggedStart(*[Create(g) for g in gs], lag_ratio=0.2), FadeIn(legend), run_time=1.6)
        self.say("Около нуля все эти функции почти совпадают с прямой y = x. Говорят: они эквивалентны x.")
        self.note(r"f\sim g\ (x\to a)\iff \lim\frac fg=1;\qquad f=o(g)\iff\lim\frac fg=0",
                  r"x\to0:\ \sin x\sim\operatorname{tg}x\sim\arcsin x\sim\operatorname{arctg}x\sim x",
                  r"\ln(1+x)\sim x,\quad e^x-1\sim x,\quad a^x-1\sim x\ln a",
                  r"1-\cos x\sim\frac{x^2}{2},\quad (1+x)^\alpha-1\sim\alpha x",
                  r"\text{заменять можно в произведении и частном, НЕ в сумме}",
                  pos=RIGHT * 2.3 + UP * 0.3, size=28, max_w=9.3)
        self.say("Вместо x можно подставлять любую бесконечно малую: sin 5x ~ 5x, ln(1 + x²) ~ x².")
        self.clear_all()
        self.example(r"\lim_{x\to0}\frac{\ln(1+3x)}{\sin 2x}", [
            (r"\lim_{x\to0}\frac{3x}{2x}=\frac32", "ln(1 + 3x) ~ 3x, sin 2x ~ 2x.")])
        self.example(r"\lim_{x\to0}\frac{\operatorname{tg}x-\sin x}{x^3}", [
            (r"\lim\frac{\operatorname{tg}x\,(1-\cos x)}{x^3}", "Ловушка: заменить tg x − sin x на x − x = 0 нельзя (это сумма)! Выносим tg x."),
            (r"\lim\frac{x\cdot\frac{x^2}{2}}{x^3}=\frac12", "Теперь произведение — заменяем.")])
        self.practice(r"\lim_{x\to0}\frac{\operatorname{arctg}4x}{e^{2x}-1}=\ ?", r"2", [r"\frac{4x}{2x}"])

    def c09_continuity(self):
        self.chapter(9, "Непрерывность и разрывы")
        axs = VGroup(*[std_axes([-2, 2, 1], [-2, 3, 1], 3.6, 2.3, nums=False) for _ in range(3)]).arrange(RIGHT, buff=0.6)
        place(axs.move_to(UP * 2.0))
        a1, a2, a3 = axs
        g1 = VGroup(curve(a1, lambda x: 0.5 * x + 1, -2, 2), hole(a1.c2p(0, 1)), Dot(a1.c2p(0, 2.3), color=C_F))
        g2 = VGroup(curve(a2, lambda x: 0.4 * x, -2, -0.01), curve(a2, lambda x: 0.4 * x + 1.5, 0.01, 2),
                    Dot(a2.c2p(0, 1.5), color=C_F), hole(a2.c2p(0, 0)))
        g3 = curve(a3, lambda x: 1 / x, -2, 2, n=800)
        ts = VGroup(T("устранимый", 26), T("I рода (скачок)", 26), T("II рода", 26))
        for t, a in zip(ts, axs):
            t.next_to(a, DOWN, buff=0.2)
        self.play(Create(axs), Create(g1), Create(g2), Create(g3), FadeIn(ts), run_time=1.3)
        self.note(r"f\text{ непрерывна в }a\iff\lim_{x\to a}f(x)=f(a)",
                  r"\text{устранимый: предел есть, но}\ne f(a)\text{ или }f(a)\text{ нет}",
                  r"\text{I рода: односторонние конечны, но различны}",
                  r"\text{II рода: хотя бы один односторонний}=\infty\text{ или не существует}",
                  pos=DOWN * 1.55, size=26, max_h=2.2)
        self.say("Непрерывность = три условия: f(a) определено, предел существует, и они равны. "
                 "Сломалось одно — разрыв. Тип разрыва определяем по односторонним пределам.")
        self.clear_all()
        ax = place(std_axes([0, 2, 0.5], [-1.5, 2, 1], 6.0, 4.2, nums=False).move_to(LEFT * 3.3 + UP * 0.4))
        f = lambda x: x ** 3 + x - 1
        g = curve(ax, f, 0, 1.4)
        self.play(Create(ax), Create(g), run_time=0.8)
        lo, hi = 0.0, 1.0
        brs = VGroup()
        for i in range(4):
            m = (lo + hi) / 2
            seg = Line(ax.c2p(lo, -0.1 * i - 0.1), ax.c2p(hi, -0.1 * i - 0.1), color=C_A, stroke_width=5)
            brs.add(seg)
            self.play(Create(seg), run_time=0.35)
            if f(m) > 0:
                hi = m
            else:
                lo = m
        self.note(r"\text{Больцано–Коши: } f\in C[a;b],\ f(a)f(b)<0",
                  r"\Rightarrow\ \exists c\in(a;b):\ f(c)=0",
                  r"\text{Вейерштрасс: } f\in C[a;b]\Rightarrow f \text{ достигает } \max \text{ и } \min",
                  pos=RIGHT * 3.4 + UP * 0.6, size=28, max_w=7.0)
        self.say("x³ + x − 1: f(0) = −1 < 0, f(1) = 1 > 0 ⇒ корень в (0; 1). Делим отрезок пополам "
                 "и оставляем половину со сменой знака — метод бисекции, корень ≈ 0,68.")
        self.clear_all()
        self.practice(r"\text{Тип разрыва в 0: }\ (a)\ \frac{\sin x}{x}\quad(b)\ e^{1/x}", r"(a)\ \text{устранимый},\ (b)\ \text{II рода}",
                      [r"(b):\ \lim_{x\to0+}e^{1/x}=+\infty,\quad \lim_{x\to0-}e^{1/x}=0"])

    # ═════════════════════════════ ЧАСТЬ III ═════════════════════════════
    def c10_derivative(self):
        self.chapter(10, "Производная и касательная")
        ax = place(std_axes([-1, 3, 1], [-1, 6, 1], 5.6, 4.6).move_to(LEFT * 3.6 + UP * 0.3))
        f = lambda x: x ** 2
        g = curve(ax, f, -1, 2.4)
        self.play(Create(ax), Create(g), run_time=0.7)
        h = ValueTracker(1.3)

        def sec():
            hv = h.get_value()
            p, q = ax.c2p(1, 1), ax.c2p(1 + hv, f(1 + hv))
            d = (q - p) / np.linalg.norm(q - p)
            return VGroup(Line(p - 2.2 * d, p + 3.2 * d, color=C_A, stroke_width=3), Dot(p), Dot(q))

        s = always_redraw(sec)
        kv = live(lambda: (f(1 + h.get_value()) - 1) / h.get_value(), lambda m: m.move_to(RIGHT * 3.4 + UP * 2.9),
                  "наклон секущей = {}", color=C_A)
        self.play(FadeIn(s), FadeIn(kv), run_time=0.5)
        self.play(h.animate.set_value(0.01), run_time=2.2)
        self.note(r"f'(a)=\lim_{h\to0}\frac{f(a+h)-f(a)}{h}",
                  r"\text{касательная: } y=f(a)+f'(a)(x-a)",
                  r"\text{нормаль: } y=f(a)-\frac{1}{f'(a)}(x-a)",
                  r"\text{дифференцируема}\Rightarrow\text{непрерывна (обратно неверно: } |x|)",
                  pos=RIGHT * 3.0 + DOWN * 0.5, size=28, max_w=7.8)
        self.say("Производная — предел наклонов секущих = наклон касательной = мгновенная скорость.")
        self.clear_all()
        self.example(r"(x^2)'\big|_{x=1}", [
            (r"\lim_{h\to0}\frac{(1+h)^2-1}{h}", None),
            (r"\lim_{h\to0}(2+h)=2", "Касательная: y = 1 + 2(x − 1) = 2x − 1.")])
        self.practice(r"\text{Касательная к } y=\sqrt x \text{ в } x=4", r"y=\frac x4+1",
                      [r"y'=\frac{1}{2\sqrt x},\ y'(4)=\frac14,\ y=2+\frac14(x-4)"])

    def c11_rules(self):
        self.chapter(11, "Правила и таблица производных")
        self.note(r"(u\pm v)'=u'\pm v',\qquad (Cu)'=Cu'",
                  r"(uv)'=u'v+uv'",
                  r"\left(\frac uv\right)'=\frac{u'v-uv'}{v^2}",
                  r"\big(f(g(x))\big)'=f'(g(x))\cdot g'(x)",
                  pos=UP * 0.8, size=38)
        self.clear_all()
        self.example(r"(\sin x)'", [
            (r"\lim_{h\to0}\frac{\sin(x+h)-\sin x}{h}", None),
            (r"\lim\frac{2\cos\left(x+\frac h2\right)\sin\frac h2}{h}", "Разность синусов → произведение."),
            (r"\lim\cos\left(x+\frac h2\right)\cdot\frac{\sin\frac h2}{\frac h2}=\cos x", "1-й замечательный предел!")],
            label="Откуда берётся таблица: пример")
        left = [r"(C)'=0", r"(x^n)'=nx^{n-1}", r"(e^x)'=e^x", r"(a^x)'=a^x\ln a",
                r"(\ln x)'=\frac1x", r"(\log_a x)'=\frac{1}{x\ln a}", r"(\sin x)'=\cos x"]
        right = [r"(\cos x)'=-\sin x", r"(\operatorname{tg}x)'=\frac{1}{\cos^2x}", r"(\operatorname{ctg}x)'=-\frac{1}{\sin^2x}",
                 r"(\arcsin x)'=\frac{1}{\sqrt{1-x^2}}", r"(\arccos x)'=-\frac{1}{\sqrt{1-x^2}}",
                 r"(\operatorname{arctg}x)'=\frac{1}{1+x^2}", r"(\operatorname{arcctg}x)'=-\frac{1}{1+x^2}"]
        p1 = self.note(*left, title="Таблица производных", pos=LEFT * 3.5, size=32, max_w=6.6, wait=0.2)
        p2 = self.note(*right, title="Таблица (продолжение)", pos=RIGHT * 3.5, size=32, max_w=6.6, wait=3.5)
        self.say("Эту таблицу надо знать наизусть. Все строчки выводятся из определения и правил.")
        self.clear_all()
        self.example(r"\left(\frac{x}{x^2+1}\right)'", [
            (r"\frac{1\cdot(x^2+1)-x\cdot2x}{(x^2+1)^2}", "Правило частного."),
            (r"\frac{1-x^2}{(x^2+1)^2}", None)])
        self.practice(r"(e^x\cos x)'=\ ?", r"e^x(\cos x-\sin x)", [r"(e^x)'\cos x+e^x(\cos x)'"])

    def c12_chain(self):
        self.chapter(12, "Цепное правило")
        eq = M(r"\sin", r"\big(", r"x^2", r"\big)", size=72)
        eq[0].set_color(C_A)
        eq[2].set_color(C_F)
        eq.move_to(UP * 2.2)
        self.play(Write(eq), run_time=0.7)
        b1 = Brace(VGroup(eq[0], eq[1], eq[3]), DOWN, color=C_A)
        t1 = T("внешняя f(u) = sin u", 26, C_A).next_to(b1, DOWN, buff=0.1)
        b2 = Brace(eq[2], UP, color=C_F)
        t2 = T("внутренняя u = x²", 26, C_F).next_to(b2, UP, buff=0.1)
        self.play(GrowFromCenter(b1), FadeIn(t1), GrowFromCenter(b2), FadeIn(t2), run_time=0.7)
        res = M(r"\big(\sin x^2\big)'=", r"\cos(x^2)", r"\cdot", r"2x", size=48).next_to(t1, DOWN, buff=0.5)
        res[1].set_color(C_A)
        res[3].set_color(C_F)
        self.play(Write(res), run_time=0.8)
        self.say("Производная внешней функции (внутренность не трогаем!) умножить на производную внутренней.")
        self.clear_all()
        self.note(r"\big(f(g(x))\big)'=f'(g(x))\cdot g'(x),\qquad \frac{dy}{dx}=\frac{dy}{du}\cdot\frac{du}{dx}",
                  r"\big((3x+1)^5\big)'=5(3x+1)^4\cdot3=15(3x+1)^4",
                  r"\big(e^{-x^2}\big)'=e^{-x^2}\cdot(-2x)",
                  r"\big(\ln\cos x\big)'=\frac{1}{\cos x}\cdot(-\sin x)=-\operatorname{tg}x",
                  r"\big(\sqrt{1+x^2}\big)'=\frac{1}{2\sqrt{1+x^2}}\cdot 2x=\frac{x}{\sqrt{1+x^2}}",
                  r"\big(\sin^3 2x\big)'=3\sin^2 2x\cdot\cos2x\cdot 2",
                  pos=UP * 0.4, size=34, wait=3.0)
        self.say("Цепочка может быть длинной: sin³(2x) — это куб от синуса от 2x. Снимаем слои снаружи "
                 "внутрь и перемножаем.")
        self.clear_all()
        self.practice(r"\big(\operatorname{arctg}e^{x}\big)'=\ ?", r"\frac{e^x}{1+e^{2x}}", [r"\frac{1}{1+(e^x)^2}\cdot e^x"])

    def c13_implicit(self):
        self.chapter(13, "Неявные, обратные, параметрические")
        ax = place(std_axes([-6, 6, 2], [-6, 6, 2], 4.8, 4.8).move_to(LEFT * 4.1 + UP * 0.35))
        circ = Circle(radius=abs(ax.c2p(5, 0)[0] - ax.c2p(0, 0)[0]), color=C_F).move_to(ax.c2p(0, 0))
        p = Dot(ax.c2p(3, 4), color=C_A)
        tl = tangent(ax, lambda x: np.sqrt(25 - x ** 2), lambda x: -x / np.sqrt(25 - x ** 2), 3, 3.2)
        self.play(Create(ax), Create(circ), run_time=0.7)
        self.play(FadeIn(p), Create(tl), run_time=0.6)
        self.derive(r"x^2+y^2=25", [
            (r"2x+2y\,y'=0", "Дифференцируем обе части по x, помня, что y = y(x): (y²)' = 2y·y'."),
            (r"y'=-\frac xy,\quad y'(3;4)=-\frac34", None)],
            pos=RIGHT * 2.5 + UP * 0.6, max_w=8.0, eq=r"\Rightarrow")
        self.clear_all()
        self.note(r"\text{неявная } F(x,y)=0:\ \text{дифференцируем по } x,\ y=y(x),\ \text{выражаем } y'",
                  r"\text{логарифмическая: } y=u^v\Rightarrow \ln y=v\ln u\Rightarrow \frac{y'}{y}=(v\ln u)'",
                  r"\text{обратная: } \big(f^{-1}\big)'(y)=\frac{1}{f'(x)}",
                  r"\text{параметрическая: } x=x(t),\ y=y(t):\quad y'_x=\frac{y'_t}{x'_t}",
                  pos=UP * 0.6, size=32)
        self.clear_all()
        self.example(r"y=x^x", eq=r"\Rightarrow", steps=[
            (r"\ln y=x\ln x", "Логарифмируем: показатель спускается множителем."),
            (r"\frac{y'}{y}=\ln x+1", "Дифференцируем: (ln y)' = y'/y."),
            (r"y'=x^x(\ln x+1)", None)], label="Пример: (xˣ)'")
        self.example(r"y=\arcsin x", eq=r"\Rightarrow", steps=[
            (r"x=\sin y", None),
            (r"1=\cos y\cdot y'", "Дифференцируем x = sin y по x как неявную функцию."),
            (r"y'=\frac{1}{\cos y}=\frac{1}{\sqrt{1-\sin^2y}}=\frac{1}{\sqrt{1-x^2}}", "cos y ≥ 0 на [−π/2; π/2].")],
            label="Пример: (arcsin x)'")
        self.practice(r"x^3+y^3=6xy.\quad y'=\ ?", r"y'=\frac{2y-x^2}{y^2-2x}", [r"3x^2+3y^2y'=6y+6xy'"])

    def c14_approx(self):
        self.chapter(14, "Высшие производные, дифференциал, приближения")
        ax = place(std_axes([0, 3, 1], [0, 3, 1], 5.0, 4.6, nums=False).move_to(LEFT * 3.9 + UP * 0.3))
        f = lambda x: 0.3 * x ** 2 + 0.3
        g = curve(ax, f, 0, 2.9)
        a, dx = 1.2, 1.1
        p, q = ax.c2p(a, f(a)), ax.c2p(a + dx, f(a + dx))
        c = ax.c2p(a + dx, f(a))
        tq = ax.c2p(a + dx, f(a) + 0.6 * a * dx)
        tl = Line(ax.c2p(0.3, f(a) - 0.6 * a * 0.9), ax.c2p(2.7, f(a) + 0.6 * a * 1.5), color=C_A, stroke_width=3)
        parts = VGroup(Line(p, c, color=C_H, stroke_width=4), Line(c, tq, color=C_A, stroke_width=6),
                       Line(c, q, color=C_DY, stroke_width=3).shift(RIGHT * 0.12))
        labs = VGroup(M(r"\Delta x", size=26, color=C_H).next_to(parts[0], DOWN, buff=0.1),
                      M("dy", size=26, color=C_A).next_to(parts[1], LEFT, buff=0.1),
                      M(r"\Delta y", size=26, color=C_DY).next_to(parts[2], RIGHT, buff=0.1))
        self.play(Create(ax), Create(g), Create(tl), run_time=0.8)
        self.play(Create(parts), FadeIn(labs), run_time=0.8)
        self.note(r"f''=(f')',\quad f^{(n)}=\big(f^{(n-1)}\big)'\qquad a(t)=v'(t)=s''(t)",
                  r"dy=f'(x)\,dx\quad(\text{рост по касательной})",
                  r"f(a+\Delta x)\approx f(a)+f'(a)\,\Delta x",
                  r"\text{Ньютон: } x_{n+1}=x_n-\frac{f(x_n)}{f'(x_n)}",
                  pos=RIGHT * 2.8 + UP * 0.4, size=28, max_w=8.0)
        self.say("Δy — настоящий прирост, dy — прирост по касательной. При малых Δx они почти равны: "
                 "на этом стоят приближённые вычисления.")
        self.clear_all()
        self.example(r"(xe^x)''", [
            (r"(e^x+xe^x)'", None), (r"2e^x+xe^x", None)])
        self.example(r"\sqrt{4{,}1}", [
            (r"\sqrt4+\frac{1}{2\sqrt4}\cdot0{,}1", "f(x) = √x, a = 4, Δx = 0,1."),
            (r"2{,}025\quad(\text{точно } 2{,}02485\ldots)", None)], label="Пример: приближённо")
        self.example(r"\sqrt2:\ x^2-2=0,\ x_0=2", eq=r"\ \to\ ", steps=[
            (r"x_1=2-\frac{2}{4}=1{,}5", "Метод Ньютона: идём по касательной до оси x."),
            (r"x_2=1{,}5-\frac{0{,}25}{3}=1{,}41667", None),
            (r"x_3=1{,}414216\ \ (\text{ошибка }<10^{-5})", "Точность удваивает число верных знаков на каждом шаге.")],
            label="Пример: метод Ньютона")
        self.practice(r"\sqrt[3]{8{,}12}\approx\ ?", r"2{,}01", [r"2+\frac{1}{3\cdot 8^{2/3}}\cdot0{,}12=2+\frac{0{,}12}{12}"])

    def c15_related(self):
        self.chapter(15, "Связанные скорости")
        O = LEFT * 4.6 + DOWN * 2.2
        s = 0.8
        wall = Line(O, O + UP * 4.6, color=GREY_B, stroke_width=4)
        floor = Line(O, O + RIGHT * 5, color=GREY_B, stroke_width=4)
        x = ValueTracker(1.5)
        lad = always_redraw(lambda: Line(O + RIGHT * x.get_value() * s,
                                         O + UP * np.sqrt(25 - x.get_value() ** 2) * s, color=C_A, stroke_width=6))
        xl = always_redraw(lambda: M("x", size=30, color=C_H).next_to(O + RIGHT * x.get_value() * s / 2, DOWN, buff=0.1))
        yl = always_redraw(lambda: M("y", size=30, color=C_DY).next_to(O + UP * np.sqrt(25 - x.get_value() ** 2) * s / 2, LEFT, buff=0.1))
        self.play(Create(wall), Create(floor), FadeIn(lad), FadeIn(xl), FadeIn(yl), run_time=0.8)
        vals = VGroup(
            live(lambda: x.get_value(), lambda m: m.move_to(LEFT * 0.8 + UP * 2.9), "x = {} м", d=2, color=C_H),
            live(lambda: np.sqrt(25 - x.get_value() ** 2), lambda m: m.move_to(LEFT * 0.8 + UP * 2.3), "y = {} м", d=2, color=C_DY),
            live(lambda: -x.get_value() / np.sqrt(25 - x.get_value() ** 2), lambda m: m.move_to(LEFT * 0.8 + UP * 1.7),
                 "dy/dt = {} м/с", d=2, color=C_A))
        self.add(vals)
        self.say("Лестница 5 м, низ оттягивают со скоростью 1 м/с. Как быстро опускается верх?")
        self.play(x.animate.set_value(3), run_time=2, rate_func=linear)
        self.derive(r"x^2+y^2=25", [
            (r"2x\,x'+2y\,y'=0", "Дифференцируем по времени t."),
            (r"y'=-\frac{x\,x'}{y}=-\frac{3\cdot1}{4}=-0{,}75\ \text{м/с}", "Подставляем момент x = 3, y = 4.")],
            pos=RIGHT * 3.2 + DOWN * 0.4, max_w=7.2, eq=r"\Rightarrow")
        self.play(x.animate.set_value(4.6), run_time=1.8, rate_func=linear)
        self.say("Чем ниже верх, тем быстрее он падает: при x → 5 скорость → ∞.")
        self.clear_all()
        self.note(r"1)\ \text{рисунок и обозначения (всё, что меняется, — функции } t)",
                  r"2)\ \text{уравнение связи между величинами}",
                  r"3)\ \text{дифференцируем по } t",
                  r"4)\ \text{только теперь подставляем числа данного момента}",
                  pos=UP * 0.6, size=32, title="Алгоритм")
        self.clear_all()
        self.practice(r"V=\tfrac43\pi r^3,\ \ V'=100\ \tfrac{\text{см}^3}{\text{с}},\ r=5.\quad r'=\ ?",
                      r"r'=\frac1\pi\approx0{,}32\ \text{см/с}", [r"V'=4\pi r^2\,r'\ \Rightarrow\ r'=\frac{100}{4\pi\cdot25}"])

    # ═════════════════════════════ ЧАСТЬ IV ══════════════════════════════
    def c16_mvt(self):
        self.chapter(16, "Теоремы о среднем")
        ax = place(std_axes([0, 5, 1], [0, 4, 1], 6.2, 4.4, nums=False).move_to(LEFT * 3.4 + UP * 0.3))
        f = lambda x: 0.15 * x ** 3 - 0.9 * x ** 2 + 1.5 * x + 1
        fp = lambda x: 0.45 * x ** 2 - 1.8 * x + 1.5
        a, b = 0.3, 4.6
        m = (f(b) - f(a)) / (b - a)
        cs = [r for r in np.roots([0.45, -1.8, 1.5 - m]) if a < r.real < b and abs(r.imag) < 1e-9]
        c = float(np.real(cs[0])) if cs else 2.5
        g = curve(ax, f, 0, 4.9)
        sec = Line(ax.c2p(a, f(a)), ax.c2p(b, f(b)), color=C_PINK, stroke_width=3)
        self.play(Create(ax), Create(g), Create(sec), FadeIn(Dot(ax.c2p(a, f(a)))), FadeIn(Dot(ax.c2p(b, f(b)))), run_time=0.9)
        t = ValueTracker(a + 0.1)
        tl = always_redraw(lambda: tangent(ax, f, fp, t.get_value(), 3.0))
        self.add(tl)
        self.play(t.animate.set_value(c), run_time=2)
        self.play(Indicate(tl, color=C_A), run_time=0.6)
        self.note(r"\text{Ферма: экстремум внутри и } \exists f'(c)\Rightarrow f'(c)=0",
                  r"\text{Ролль: } f(a)=f(b)\Rightarrow\exists c:\ f'(c)=0",
                  r"\text{Лагранж: } \exists c\in(a;b):\ f'(c)=\frac{f(b)-f(a)}{b-a}",
                  r"\text{Коши: } \frac{f(b)-f(a)}{g(b)-g(a)}=\frac{f'(c)}{g'(c)}",
                  r"f'\equiv0\Rightarrow f=\text{const};\quad f'>0\Rightarrow f\uparrow",
                  pos=RIGHT * 3.2 + UP * 0.2, size=28, max_w=7.4)
        self.say("Лагранж: где-то на отрезке касательная параллельна секущей — мгновенная скорость "
                 "хоть раз равна средней. Отсюда все признаки монотонности.")
        self.clear_all()
        self.example(r"|\sin a-\sin b|", [
            (r"|\cos c|\cdot|a-b|\le|a-b|", "Лагранж для sin на [a; b]: sin a − sin b = cos c · (a − b), а |cos c| ≤ 1.")],
            label="Пример: неравенство")
        self.practice(r"f=x^2 \text{ на } [0;2].\ \ c\ \text{из теоремы Лагранжа}=\ ?", r"c=1",
                      [r"2c=\frac{4-0}{2-0}=2"])

    def c17_lhopital(self):
        self.chapter(17, "Правило Лопиталя")
        self.note(r"\left[\tfrac00\right]\text{ или }\left[\tfrac{\infty}{\infty}\right]:\quad \lim\frac{f}{g}=\lim\frac{f'}{g'}",
                  r"\text{(если правый предел существует)}",
                  r"0\cdot\infty\ \to\ \frac{f}{1/g};\qquad 1^\infty,\ 0^0,\ \infty^0\ \to\ f^g=e^{g\ln f}",
                  pos=UP * 1.5, size=34)
        self.say("Почему работает: около точки a обе функции почти линейны: f ≈ f'(a)(x − a), g ≈ g'(a)(x − a). "
                 "Отношение → отношение наклонов.")
        self.clear_all()
        self.example(r"\lim_{x\to0}\frac{e^x-1-x}{x^2}", [
            (r"\lim\frac{e^x-1}{2x}", "0/0 → дифференцируем числитель и знаменатель ОТДЕЛЬНО (это не производная дроби!)."),
            (r"\lim\frac{e^x}{2}=\frac12", "Снова 0/0 — применяем ещё раз.")])
        self.example(r"\lim_{x\to0+}x\ln x", [
            (r"\lim\frac{\ln x}{1/x}", "0·∞ → переписываем как ∞/∞."),
            (r"\lim\frac{1/x}{-1/x^2}=\lim(-x)=0", None)])
        self.example(r"\lim_{x\to0+}x^x", [
            (r"e^{\lim x\ln x}", "0⁰ → через экспоненту."), (r"e^0=1", "Предел показателя посчитали выше.")])
        self.say("Перед применением ВСЕГДА проверяй, что это 0/0 или ∞/∞. Например, (x + 1)/x при x → 0 — не неопределённость.")
        self.practice(r"\lim_{x\to0}\frac{x-\sin x}{x^3}=\ ?", r"\frac16",
                      [r"\frac{1-\cos x}{3x^2}\to\frac{\sin x}{6x}\to\frac16"])

    def c18_extrema(self):
        self.chapter(18, "Монотонность и экстремумы")
        ax = place(std_axes([-2.5, 2.5, 1], [-3, 3, 1], 5.8, 4.0).move_to(LEFT * 3.5 + UP * 1.0))
        f = lambda x: x ** 3 - 3 * x
        g = curve(ax, f, -2.3, 2.3)
        self.play(Create(ax), Create(g), run_time=0.8)
        nl = Line(LEFT * 6.3 + DOWN * 1.9, LEFT * 0.7 + DOWN * 1.9, color=GREY_B)
        xs = {-1: nl.point_from_proportion(0.33), 1: nl.point_from_proportion(0.67)}
        marks = VGroup(*[VGroup(Dot(p, color=C_A), M(str(k), size=26).next_to(p, DOWN, buff=0.12)) for k, p in xs.items()])
        signs = VGroup(*[M(s, size=34, color=c).move_to(nl.point_from_proportion(t) + UP * 0.35)
                         for s, c, t in [("+", C_GOOD, 0.15), ("-", C_BAD, 0.5), ("+", C_GOOD, 0.85)]])
        arrows = VGroup(*[T(s, 30, c).move_to(nl.point_from_proportion(t) + DOWN * 0.45)
                          for s, c, t in [("↗", C_GOOD, 0.15), ("↘", C_BAD, 0.5), ("↗", C_GOOD, 0.85)]])
        fpl = M("f'", size=30).next_to(nl, LEFT, buff=0.1)
        self.play(Create(nl), FadeIn(marks), Write(fpl), run_time=0.6)
        self.play(FadeIn(signs), FadeIn(arrows), run_time=0.6)
        self.play(FadeIn(Dot(ax.c2p(-1, 2), color=C_A)), FadeIn(Dot(ax.c2p(1, -2), color=C_A)), run_time=0.4)
        self.note(r"f'>0\Rightarrow f\uparrow,\qquad f'<0\Rightarrow f\downarrow",
                  r"\text{критические точки: } f'=0\ \text{или}\ \nexists f'",
                  r"f':\ +\to-\ \ \text{max};\qquad -\to+\ \ \text{min}",
                  r"f=x^3-3x:\ f'=3(x-1)(x+1)",
                  r"\max:\ f(-1)=2,\qquad \min:\ f(1)=-2",
                  pos=RIGHT * 3.3 + UP * 0.3, size=28, max_w=7.0)
        self.say("Схема: f' → критические точки → знаки f' на интервалах → где + меняется на − — максимум, "
                 "где − на + — минимум.")
        self.clear_all()
        self.practice(r"f=xe^{-x}.\ \ \text{Экстремумы?}", r"\max\ \text{в } x=1,\ f(1)=\frac1e",
                      [r"f'=e^{-x}(1-x):\ +\ \text{до 1},\ -\ \text{после}"])

    def c19_convexity(self):
        self.chapter(19, "Выпуклость и перегиб")
        ax = place(std_axes([-2.5, 2.5, 1], [-3, 3, 1], 6.0, 4.6).move_to(LEFT * 3.4 + UP * 0.3))
        f = lambda x: x ** 3 - 3 * x
        fp = lambda x: 3 * x ** 2 - 3
        g = curve(ax, f, -2.3, 2.3)
        self.play(Create(ax), Create(g), run_time=0.7)
        t = ValueTracker(-2)
        tl = always_redraw(lambda: tangent(ax, f, fp, t.get_value(), 2.6,
                                           color=C_PINK if t.get_value() < 0 else C_GOOD))
        self.add(tl)
        self.play(t.animate.set_value(2), run_time=3, rate_func=linear)
        self.play(FadeIn(Dot(ax.c2p(0, 0), color=C_A, radius=0.1)), run_time=0.3)
        self.note(r"f''>0:\ \text{выпукла вниз } \cup\ (\text{касательная под графиком})",
                  r"f''<0:\ \text{выпукла вверх } \cap",
                  r"\text{перегиб: } f'' \text{ меняет знак}",
                  r"f'(c)=0,\ f''(c)>0\Rightarrow\min;\ \ f''(c)<0\Rightarrow\max",
                  r"x^3-3x:\ f''=6x,\ \text{перегиб в } (0;0)",
                  pos=RIGHT * 3.1 + UP * 0.3, size=28, max_w=7.4)
        self.say("Левее нуля касательная над графиком (∩), правее — под графиком (∪). В точке перегиба "
                 "касательная пересекает график.")
        self.clear_all()
        self.practice(r"f=x^4-6x^2.\ \ \text{Точки перегиба?}", r"x=\pm1",
                      [r"f''=12x^2-12=0,\ \text{знак меняется}"])

    def c20_sketch(self):
        self.chapter(20, "Полное исследование функции")
        steps = [r"1)\ D:\ x\ne1", r"2)\ \text{нули: } x=0;\ \text{ни чётная, ни нечётная}",
                 r"3)\ \text{верт. асимптота } x=1;\ \ \frac{x^2}{x-1}=x+1+\frac{1}{x-1}\Rightarrow y=x+1",
                 r"4)\ f'=\frac{x(x-2)}{(x-1)^2}:\ \max\ (0;0),\ \min\ (2;4)",
                 r"5)\ f''=\frac{2}{(x-1)^3}:\ \cap \text{ при } x<1,\ \cup \text{ при } x>1"]
        title = M(r"f(x)=\frac{x^2}{x-1}", size=40, color=C_F).to_edge(UP, buff=0.5)
        self.play(Write(title), run_time=0.6)
        rows = VGroup(*[M(s, size=30) for s in steps]).arrange(DOWN, aligned_edge=LEFT, buff=0.28)
        place(rows.next_to(title, DOWN, buff=0.35))
        for r in rows:
            self.play(FadeIn(r, shift=RIGHT * 0.2), run_time=0.5)
            self.wait(0.9)
        self.say("Порядок всегда один: область, нули и симметрия, асимптоты, f' (монотонность, экстремумы), "
                 "f'' (выпуклость, перегибы). Потом рисуем.")
        self.clear_all()
        ax = place(std_axes([-3, 5, 1], [-6, 10, 2], 7.0, 5.6).move_to(UP * 0.35))
        f = lambda x: x ** 2 / (x - 1)
        va = DashedLine(ax.c2p(1, -6), ax.c2p(1, 10), color=C_PINK)
        oa = DashedLine(ax.c2p(-3, -2), ax.c2p(5, 6), color=C_A)
        self.play(Create(ax), Create(va), Create(oa), run_time=0.8)
        pts = VGroup(Dot(ax.c2p(0, 0), color=C_A), Dot(ax.c2p(2, 4), color=C_A))
        self.play(FadeIn(pts), run_time=0.3)
        self.play(Create(curve(ax, f, -3, 5, n=900)), run_time=1.6)
        self.say("Асимптоты задают «скелет», экстремумы — опорные точки, выпуклость — форму. График готов.")
        self.clear_all()

    def c21_optim(self):
        self.chapter(21, "Оптимизация")
        s = 0.3
        x = ValueTracker(1.0)
        C0 = LEFT * 4.3 + UP * 0.3

        def sheet():
            xv = x.get_value()
            sq = Square(12 * s).move_to(C0).set_stroke(GREY_B, 2).set_fill(C_F, 0.25)
            cs = VGroup(*[Square(xv * s).set_fill(C_BAD, 0.7).set_stroke(width=0)
                          .move_to(C0 + np.array([sx * (6 - xv / 2) * s, sy * (6 - xv / 2) * s, 0]))
                          for sx in (-1, 1) for sy in (-1, 1)])
            inner = Square((12 - 2 * xv) * s).move_to(C0).set_stroke(C_A, 2, opacity=0.8).set_fill(opacity=0)
            return VGroup(sq, cs, inner)

        sh = always_redraw(sheet)
        lab = T("лист 12×12, вырезаем углы x×x", 24, GREY_A).next_to(C0 + DOWN * 1.8, DOWN, buff=0.1)
        V = lambda v: v * (12 - 2 * v) ** 2
        ax = place(std_axes([0, 6, 1], [0, 140, 20], 5.6, 4.2, font=18).move_to(RIGHT * 3.3 + UP * 0.6))
        g = curve(ax, V, 0, 6, color=C_A)
        dot = always_redraw(lambda: Dot(ax.c2p(x.get_value(), V(x.get_value())), color=C_A))
        vv = live(lambda: V(x.get_value()), lambda m: m.next_to(ax, UP, buff=0.05), "V = {}", d=1, size=28)
        self.play(FadeIn(sh), FadeIn(lab), Create(ax), Create(g), FadeIn(dot), FadeIn(vv), run_time=1)
        self.play(x.animate.set_value(4.5), run_time=1.6)
        self.play(x.animate.set_value(2), run_time=1.4)
        self.derive(r"V(x)=x(12-2x)^2", [
            (r"V'=(12-2x)(12-6x)=0\ \Rightarrow\ x=2,\ \ V_{\max}=128", None)], size=30, pos=DOWN * 2.1, eq=r"\Rightarrow")
        self.say("Коробка без крышки: V' = 0 при x = 2 (x = 6 — вырожденная коробка). Максимальный объём 128.")
        self.clear_all()
        self.note(r"\text{наибольшее/наименьшее на } [a;b]:",
                  r"1)\ \text{критические точки внутри } (a;b)",
                  r"2)\ \text{значения в них и на концах } f(a),\ f(b)",
                  r"3)\ \text{выбрать max и min из этого списка}",
                  r"f=x^3-3x \text{ на } [-2;3]:\ f(-2)=-2,\ f(-1)=2,\ f(1)=-2,\ f(3)=18",
                  pos=UP * 0.6, size=32, title="Алгоритм")
        self.clear_all()
        self.practice(r"\min_{x>0}\left(x+\frac4x\right)=\ ?", r"4\ \ (\text{при } x=2)", [r"1-\frac{4}{x^2}=0\Rightarrow x=2"])

    def c22_taylor(self):
        self.chapter(22, "Формула Тейлора")
        ax = place(std_axes([-4, 4, 1], [-1, 8, 1], 6.2, 4.8).move_to(LEFT * 3.4 + UP * 0.3))
        g = curve(ax, np.exp, -4, 4, color=C_F, width=5)
        self.play(Create(ax), Create(g), run_time=0.7)
        from math import factorial
        P = lambda n: (lambda x: sum(x ** k / factorial(k) for k in range(n + 1)))
        cur = curve(ax, P(0), -4, 4, color=C_A, width=3)
        nlab = T("n = 0", 30, C_A).move_to(RIGHT * 3.4 + UP * 2.9)
        self.play(Create(cur), FadeIn(nlab), run_time=0.5)
        for n in range(1, 7):
            new = curve(ax, P(n), -4, 4, color=C_A, width=3)
            self.play(ReplacementTransform(cur, new), Transform(nlab, T(f"n = {n}", 30, C_A).move_to(nlab)), run_time=0.6)
            cur = new
        self.say("Многочлены Тейлора облепляют eˣ всё дальше от нуля: каждое слагаемое добавляет совпадение "
                 "ещё одной производной.")
        self.note(r"f(x)=\sum_{k=0}^{n}\frac{f^{(k)}(a)}{k!}(x-a)^k+R_n(x)",
                  r"R_n=\frac{f^{(n+1)}(c)}{(n+1)!}(x-a)^{n+1}\ (\text{Лагранж}),\ \ R_n=o\big((x-a)^n\big)\ (\text{Пеано})",
                  pos=RIGHT * 3.1 + DOWN * 0.5, size=26, max_w=7.6)
        self.clear_all()
        self.note(r"e^x=1+x+\frac{x^2}{2!}+\frac{x^3}{3!}+\dots+\frac{x^n}{n!}+o(x^n)",
                  r"\sin x=x-\frac{x^3}{3!}+\frac{x^5}{5!}-\dots",
                  r"\cos x=1-\frac{x^2}{2!}+\frac{x^4}{4!}-\dots",
                  r"\ln(1+x)=x-\frac{x^2}{2}+\frac{x^3}{3}-\dots",
                  r"(1+x)^\alpha=1+\alpha x+\frac{\alpha(\alpha-1)}{2!}x^2+\dots",
                  title="Маклорен (a = 0)", pos=UP * 0.4, size=34, wait=3.0)
        self.clear_all()
        self.example(r"\lim_{x\to0}\frac{x-\sin x}{x^3}", [
            (r"\lim\frac{x-\left(x-\frac{x^3}{6}+o(x^3)\right)}{x^3}", "Раскладываем sin до x³."),
            (r"\lim\frac{\frac{x^3}{6}+o(x^3)}{x^3}=\frac16", "Тот же ответ, что и по Лопиталю, — но быстрее.")])
        self.practice(r"\lim_{x\to0}\frac{\cos x-1+\frac{x^2}{2}}{x^4}=\ ?", r"\frac{1}{24}",
                      [r"\cos x=1-\frac{x^2}{2}+\frac{x^4}{24}+o(x^4)"])

    # ═════════════════════════════ ЧАСТЬ V ═══════════════════════════════
    def c23_antider(self):
        self.chapter(23, "Первообразная и таблица интегралов")
        ax = place(std_axes([-2, 2, 1], [-2, 4, 1], 5.4, 4.4).move_to(LEFT * 3.8 + UP * 0.3))
        fam = VGroup(*[curve(ax, lambda x, c=c: x ** 3 / 3 + c, -2, 2, color=C_A, width=2) for c in (-1, 0, 1, 2)])
        self.play(Create(ax), LaggedStart(*[Create(f) for f in fam], lag_ratio=0.2), run_time=1.2)
        self.note(r"F'=f\ \Rightarrow\ \int f(x)\,dx=F(x)+C",
                  r"\int(\alpha f+\beta g)\,dx=\alpha\!\int\! f\,dx+\beta\!\int\! g\,dx",
                  r"\text{проверка: продифференцируй ответ}",
                  pos=RIGHT * 3.2 + UP * 0.8, size=30, max_w=7.2)
        self.say("Первообразных бесконечно много: они отличаются на константу — графики сдвинуты по вертикали.")
        self.clear_all()
        left = [r"\int x^n dx=\frac{x^{n+1}}{n+1}+C\ (n\ne-1)", r"\int\frac{dx}{x}=\ln|x|+C", r"\int e^xdx=e^x+C",
                r"\int a^xdx=\frac{a^x}{\ln a}+C", r"\int\sin x\,dx=-\cos x+C", r"\int\cos x\,dx=\sin x+C"]
        right = [r"\int\frac{dx}{\cos^2x}=\operatorname{tg}x+C", r"\int\frac{dx}{\sin^2x}=-\operatorname{ctg}x+C",
                 r"\int\frac{dx}{1+x^2}=\operatorname{arctg}x+C", r"\int\frac{dx}{\sqrt{1-x^2}}=\arcsin x+C",
                 r"\int\frac{dx}{x^2+a^2}=\frac1a\operatorname{arctg}\frac xa+C", r"\int\frac{dx}{x^2-a^2}=\frac{1}{2a}\ln\left|\frac{x-a}{x+a}\right|+C"]
        self.note(*left, title="Таблица интегралов", pos=LEFT * 3.5, size=30, max_w=6.8, wait=0.2)
        self.note(*right, title="Таблица (продолжение)", pos=RIGHT * 3.5, size=30, max_w=6.8, wait=3.5)
        self.clear_all()
        self.example(r"\int\left(3x^2-\frac4x+\sqrt x\right)dx", [
            (r"x^3-4\ln|x|+\frac{x^{3/2}}{3/2}+C", "Линейность + таблица; √x = x^{1/2}."),
            (r"x^3-4\ln|x|+\frac23x\sqrt x+C", None)])
        self.practice(r"\int\left(2\cos x-\frac{1}{1+x^2}\right)dx=\ ?", r"2\sin x-\operatorname{arctg}x+C")

    def c24_techniques(self):
        self.chapter(24, "Замена переменной и по частям")
        self.note(r"\int f\big(g(x)\big)g'(x)\,dx=\int f(u)\,du,\qquad u=g(x)",
                  r"\int f(kx+b)\,dx=\frac1k F(kx+b)+C",
                  r"\int u\,dv=uv-\int v\,du",
                  r"u:\ \text{ln, arc-функции, многочлен};\quad dv:\ e^x,\ \sin,\ \cos",
                  pos=UP * 0.9, size=34)
        self.say("Замена — это цепное правило наоборот. По частям — правило произведения наоборот: "
                 "(uv)' = u'v + uv'.")
        self.clear_all()
        self.example(r"\int 2x\cos(x^2)\,dx", [
            (r"\int\cos u\,du\quad(u=x^2,\ du=2x\,dx)", "Видим внутреннюю функцию x² и её производную 2x рядом."),
            (r"\sin u+C=\sin(x^2)+C", None)], label="Замена")
        self.example(r"\int\frac{x\,dx}{x^2+1}", [
            (r"\frac12\int\frac{du}{u}\quad(u=x^2+1)", None), (r"\frac12\ln(x^2+1)+C", None)], label="Замена")
        self.example(r"\int xe^x\,dx", [
            (r"xe^x-\int e^x\,dx\quad(u=x,\ dv=e^xdx)", "u = x упростится при дифференцировании."),
            (r"xe^x-e^x+C", None)], label="По частям")
        self.example(r"\int\ln x\,dx", [
            (r"x\ln x-\int x\cdot\frac1x\,dx\quad(u=\ln x,\ dv=dx)", "Классический трюк: dv = dx."),
            (r"x\ln x-x+C", None)], label="По частям")
        self.practice(r"\int xe^{2x}\,dx=\ ?", r"\frac x2e^{2x}-\frac14e^{2x}+C", [r"u=x,\ v=\tfrac12e^{2x}"])

    def c25_definite(self):
        self.chapter(25, "Определённый интеграл и Ньютон–Лейбниц")
        ax = place(std_axes([0, 1, 0.5], [0, 1, 0.5], 5.0, 4.2).move_to(LEFT * 3.9 + UP * 0.4))
        f = lambda x: x ** 2
        g = curve(ax, f, 0, 1)
        self.play(Create(ax), Create(g), run_time=0.6)
        rs = riemann(ax, f, 0, 1, 4)
        self.play(FadeIn(rs), run_time=0.5)
        for n in (8, 16, 64):
            nr = riemann(ax, f, 0, 1, n)
            self.play(ReplacementTransform(rs, nr), run_time=0.6)
            rs = nr
        self.note(r"\int_a^b f\,dx=\lim_{\max\Delta x_k\to0}\sum f(\xi_k)\Delta x_k",
                  r"\frac{d}{dx}\int_a^x f(t)\,dt=f(x)\quad(\text{ОТА})",
                  r"\int_a^b f\,dx=F(b)-F(a)=F\big|_a^b",
                  r"\text{замена: меняем пределы; по частям: } uv\big|_a^b-\int_a^b v\,du",
                  pos=RIGHT * 2.8 + UP * 0.3, size=28, max_w=8.0)
        self.say("Площадь — предел сумм Римана. Основная теорема анализа связывает её с первообразной: "
                 "считаем F на концах и вычитаем.")
        self.clear_all()
        self.example(r"\int_0^1x^2dx", [(r"\frac{x^3}{3}\Big|_0^1=\frac13", None)])
        self.example(r"\int_0^1xe^x\,dx", [
            (r"(xe^x-e^x)\Big|_0^1", "Первообразная — из прошлой главы."),
            (r"(e-e)-(0-1)=1", None)])
        self.example(r"\int_0^{\sqrt\pi}x\sin(x^2)\,dx", [
            (r"\frac12\int_0^{\pi}\sin u\,du\quad(u=x^2:\ 0\to0,\ \sqrt\pi\to\pi)", "При замене пересчитываем пределы — возвращаться к x не нужно."),
            (r"\frac12(-\cos u)\Big|_0^\pi=\frac12(1+1)=1", None)])
        self.practice(r"\int_1^e\ln x\,dx=\ ?", r"1", [r"(x\ln x-x)\Big|_1^e=(e-e)-(0-1)"])

    def c26_areas(self):
        self.chapter(26, "Площадь между кривыми, среднее значение")
        ax = place(std_axes([0, 1.2, 0.5], [0, 1.2, 0.5], 5.0, 4.4).move_to(LEFT * 3.9 + UP * 0.3))
        top = curve(ax, lambda x: x, 0, 1.2, color=C_A)
        bot = curve(ax, lambda x: x ** 2, 0, 1.1, color=C_F)
        reg = area_between(ax, lambda x: x, lambda x: x ** 2, 0, 1, C_POS, 0.5)
        self.play(Create(ax), Create(top), Create(bot), run_time=0.7)
        strips = VGroup(*[Line(ax.c2p(x, x ** 2), ax.c2p(x, x), color=C_H, stroke_width=3) for x in np.linspace(0.05, 0.95, 12)])
        self.play(LaggedStart(*[Create(s) for s in strips], lag_ratio=0.1), run_time=0.8)
        self.play(FadeIn(reg), FadeOut(strips), run_time=0.5)
        self.derive(r"S", [(r"\int_0^1(x-x^2)\,dx", "Высота полоски = верхняя − нижняя; пределы — точки пересечения x = x²."),
                           (r"\frac12-\frac13=\frac16", None)], pos=RIGHT * 3.0 + UP * 1.4, max_w=7.0)
        self.note(r"S=\int_a^b\big(f_{\text{верх}}-f_{\text{низ}}\big)\,dx",
                  r"f_{\text{ср}}=\frac{1}{b-a}\int_a^b f\,dx",
                  pos=RIGHT * 3.0 + DOWN * 1.2, size=30, max_w=7.0)
        self.clear_all()
        ax = place(std_axes([0, PI, PI / 2], [0, 1.2, 0.5], 6.0, 3.6, nums=False).move_to(UP * 0.8))
        g = curve(ax, np.sin, 0, PI)
        avg = 2 / PI
        rect = Rectangle(width=ax.x_length, height=abs(ax.c2p(0, avg)[1] - ax.c2p(0, 0)[1])).set_fill(C_A, 0.3)
        rect.set_stroke(C_A, 2).move_to(ax.c2p(PI / 2, avg / 2))
        self.play(Create(ax), Create(g), run_time=0.6)
        self.play(FadeIn(rect), run_time=0.6)
        t = M(r"\sin_{\text{ср}}=\frac1\pi\int_0^\pi\sin x\,dx=\frac2\pi\approx0{,}64", size=36).next_to(ax, DOWN, buff=0.3)
        self.play(Write(t), run_time=0.7)
        self.say("Среднее значение — высота прямоугольника с той же площадью, что и под графиком.")
        self.clear_all()
        self.practice(r"S \text{ между } y=x^2 \text{ и } y=2x", r"\frac43", [r"\int_0^2(2x-x^2)\,dx=4-\frac83"])

    def c27_summary(self):
        self.chapter(27, "Итог семестра")
        chain = ["функции", "пределы", "непрерывность", "производная", "приложения", "интеграл"]
        boxes = VGroup(*[VGroup(SurroundingRectangle(T(s, 26), buff=0.15, corner_radius=0.1, color=C_F), T(s, 26)) for s in chain])
        boxes.arrange(RIGHT, buff=0.45)
        place(boxes.move_to(UP * 2.6))
        arrows = VGroup(*[Arrow(a.get_right(), b.get_left(), buff=0.05, stroke_width=3, color=GREY_B,
                                max_tip_length_to_length_ratio=0.35) for a, b in zip(boxes[:-1], boxes[1:])])
        self.play(LaggedStart(*[FadeIn(b) for b in boxes], lag_ratio=0.15), LaggedStart(*[GrowArrow(a) for a in arrows], lag_ratio=0.15), run_time=1.4)
        self.note(r"\lim_{x\to0}\frac{\sin x}{x}=1,\qquad \lim_{x\to\infty}\left(1+\frac1x\right)^x=e",
                  r"f'(x)=\lim_{h\to0}\frac{f(x+h)-f(x)}{h},\quad y=f(a)+f'(a)(x-a)",
                  r"(uv)'=u'v+uv',\quad (f(g))'=f'(g)\,g'",
                  r"\text{Лопиталь: }\lim\frac fg=\lim\frac{f'}{g'}\ \ \left[\tfrac00,\tfrac\infty\infty\right]",
                  r"f(x)=\sum\frac{f^{(k)}(a)}{k!}(x-a)^k+R_n",
                  r"\int_a^b f\,dx=F(b)-F(a),\qquad \int u\,dv=uv-\int v\,du",
                  title="Главное за семестр", pos=DOWN * 0.6, size=32, max_h=4.9, wait=4.0)
        self.say("Всё держится на одной идее — пределе. Из него выросли производная и интеграл, "
                 "а теорема Ньютона–Лейбница связала их в одно целое.")
        self.clear_all(keep_hdr=False)
        t = T("Удачи на экзамене!", 60, C_A)
        s = T("дальше: Calculus II — ряды, несобственные интегралы, дифференциальные уравнения", 26, GREY_A)
        place(VGroup(t, s).arrange(DOWN, buff=0.4))
        self.play(Write(t), FadeIn(s, shift=UP * 0.2), run_time=1.2)
        self.wait(2.5)
        self.play(FadeOut(t), FadeOut(s))


# ═════════════════════════ сборка сцен ════════════════════════════════════
PARTS = [
    ("I", "Функции", ["c01_functions", "c02_transforms"]),
    ("II", "Пределы и непрерывность", ["c03_limit_def", "c04_limit_calc", "c05_infinity", "c06_squeeze",
                                       "c07_e", "c08_equiv", "c09_continuity"]),
    ("III", "Производная", ["c10_derivative", "c11_rules", "c12_chain", "c13_implicit", "c14_approx", "c15_related"]),
    ("IV", "Приложения производной", ["c16_mvt", "c17_lhopital", "c18_extrema", "c19_convexity",
                                      "c20_sketch", "c21_optim", "c22_taylor"]),
    ("V", "Интеграл", ["c23_antider", "c24_techniques", "c25_definite", "c26_areas", "c27_summary"]),
]


class FullVideo(Story):
    def construct(self):
        self.c00_intro()
        for roman, title, chs in PARTS:
            self.part(roman, title)
            for c in chs:
                getattr(self, c)()


def _scene(name, body):
    return type(name, (Story,), {"construct": body})


C00_Intro = _scene("C00_Intro", lambda self: self.c00_intro())
for _roman, _title, _chs in PARTS:
    for _c in _chs:
        _name = "C" + _c[1:3] + "_" + "".join(w.capitalize() for w in _c[4:].split("_"))
        _first = _c == _chs[0]
        globals()[_name] = _scene(
            _name,
            (lambda c, r, t, first: (lambda self: (self.part(r, t) if first else None, getattr(self, c)())))(
                _c, _roman, _title, _first))

Part1_Functions = _scene("Part1_Functions", lambda self: (self.c00_intro(), self.part(*PARTS[0][:2]),
                                                          [getattr(self, c)() for c in PARTS[0][2]]))
Part2_Limits = _scene("Part2_Limits", lambda self: (self.part(*PARTS[1][:2]), [getattr(self, c)() for c in PARTS[1][2]]))
Part3_Derivative = _scene("Part3_Derivative", lambda self: (self.part(*PARTS[2][:2]), [getattr(self, c)() for c in PARTS[2][2]]))
Part4_Applications = _scene("Part4_Applications", lambda self: (self.part(*PARTS[3][:2]), [getattr(self, c)() for c in PARTS[3][2]]))
Part5_Integral = _scene("Part5_Integral", lambda self: (self.part(*PARTS[4][:2]), [getattr(self, c)() for c in PARTS[4][2]]))
