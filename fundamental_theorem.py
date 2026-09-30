"""
Основная теорема анализа (Ньютон–Лейбниц) — видео на Manim Community, стиль 3Blue1Brown.
Продолжение ролика derivative_limits.py (производная через пределы).

Рендер всего фильма:
    manim -qh fundamental_theorem.py FullVideo
Отдельная глава:
    manim -qm fundamental_theorem.py Ch05_FTC1

Требования: manim >= 0.18, LaTeX с кириллицей (texlive-lang-cyrillic, cm-super),
шрифт «CMU Serif» (fonts-cmu).

──────────────────────────────────────────────────────────────────────────────
ПЛАН (по методике manim-composer)
──────────────────────────────────────────────────────────────────────────────
Вопрос-крючок : по пути мы умеем находить скорость (производная). А обратно?
                Спидометр записывал скорость всю поездку — сколько мы проехали?
Аудитория     : 1 курс; знает производную, пределы, правила дифференцирования.
Ключевая идея : площадь под графиком f, как функция правого края, растёт со
                скоростью f. Поэтому площадь = разность значений первообразной.
Цвета         : f — синий; площадь (+) — синий полупрозрачный, (−) — красный;
                функция площади A(x) / первообразная F — жёлтый; Δx — зелёный.

 0. Вступление        — обратная задача: скорость → путь.
 1. Путь = площадь    — постоянная скорость → прямоугольник; ступеньки → сумма.
 2. Суммы Римана      — x² на [0;1]: левые/правые суммы, n = 4…1000, точно 1/3.
 3. Опр. интеграл     — определение как предел, смысл обозначений, знак площади,
                        свойства (линейность, аддитивность, оценка).
 4. Функция площади   — A(x) = ∫ₐˣ f; график A «прорисовывается» — похоже, A' = f.
 5. ОТА, часть 1      — доказательство: полоска между m·h и M·h, сжатие → A' = f.
 6. Ньютон–Лейбниц    — F' = f ⇒ ∫ₐᵇ f = F(b) − F(a); доказательство через
                        «разность с нулевой производной — константа»;
                        картинка: телескопическая сумма приращений F.
 7. Первообразные     — семейство F + C, таблица, примеры: ∫x², ∫sin, ∫1/x, путь.
 8. Подводные камни   — ∫₋₁¹ dx/x² ≠ −2; скачок: A непрерывна, но не гладка.
 9. Итог              — d/dx и ∫ — взаимно обратные операции.
"""

from manim import *
import numpy as np

# ───────────────────────────── стиль ──────────────────────────────────────
C_BG = "#0F1115"
C_F = BLUE_C         # подынтегральная функция
C_POS = BLUE_D       # положительная площадь
C_NEG = RED_D        # отрицательная площадь
C_A = YELLOW         # функция площади / первообразная
C_H = GREEN_C        # Δx, h
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


def T(text, size=34, color=WHITE, **kw):
    return Text(text, font=FONT, font_size=size, color=color, **kw)


def M(*tex, size=44, color=WHITE, **kw):
    return MathTex(*tex, font_size=size, color=color, **kw)


def ru(v, d=3):
    return f"{v:.{d}f}".replace(".", ",").replace("-", "−")


def wrap(text, width=64):
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


def live(getter, place, fmt="{}", d=3, size=34, color=WHITE):
    def build():
        m = T(fmt.format(ru(getter(), d)), size, color)
        place(m)
        return m

    return always_redraw(build)


def std_axes(xr, yr, xl, yl, nums=True, font=22):
    return Axes(
        x_range=xr, y_range=yr, x_length=xl, y_length=yl, tips=False,
        axis_config={"include_numbers": nums, "font_size": font, "color": GREY_B,
                     "stroke_width": 2},
    )


def area_poly(ax, f, a, b, color, opacity=0.45, n=160):
    """Закрашенная область между графиком f и осью x на [a; b]."""
    if b - a < 1e-4:
        return VMobject()
    xs = np.linspace(a, b, n)
    pts = [ax.c2p(a, 0)] + [ax.c2p(x, f(x)) for x in xs] + [ax.c2p(b, 0)]
    return Polygon(*pts, stroke_width=0, fill_color=color, fill_opacity=opacity)


def signed_area(ax, f, a, b, opacity=0.45):
    """Положительная часть — синим, отрицательная — красным."""
    return VGroup(area_poly(ax, lambda x: max(f(x), 0), a, b, C_POS, opacity),
                  area_poly(ax, lambda x: min(f(x), 0), a, b, C_NEG, opacity))


def riemann(ax, f, a, b, n, kind="left", color=C_POS, opacity=0.6):
    dx = (b - a) / n
    g = VGroup()
    for k in range(n):
        x0 = a + k * dx
        s = {"left": x0, "right": x0 + dx, "mid": x0 + dx / 2}[kind]
        y = f(s)
        p0, p1 = ax.c2p(x0, 0), ax.c2p(x0 + dx, y)
        r = Rectangle(width=abs(p1[0] - p0[0]), height=max(abs(p1[1] - p0[1]), 1e-3))
        r.set_fill(color if y >= 0 else C_NEG, opacity)
        r.set_stroke(WHITE, 1 if n <= 40 else 0.3, opacity=0.8)
        r.move_to((p0 + p1) / 2)
        g.add(r)
    return g


def hole(point, color=C_F, r=0.08):
    return Circle(radius=r, color=color, stroke_width=3).set_fill(C_BG, 1).move_to(point)


# ═════════════════════════ базовая сцена ══════════════════════════════════
class Story(Scene):
    def setup(self):
        self.cap = None

    def say(self, text, wait=None, size=27):
        lines = wrap(text)
        new = VGroup(*[T(l, size, GREY_A) for l in lines]).arrange(DOWN, buff=0.12)
        if new.width > 13.4:
            new.scale_to_fit_width(13.4)
        new.to_edge(DOWN, buff=0.28)
        anims = [FadeIn(new, shift=0.15 * UP)]
        if self.cap is not None:
            anims.append(FadeOut(self.cap, shift=0.15 * UP))
        self.play(*anims, run_time=0.6)
        self.cap = new
        self.wait(wait if wait is not None else 1.0 + 0.065 * len(text))

    def unsay(self):
        if self.cap is not None:
            self.play(FadeOut(self.cap), run_time=0.4)
            self.cap = None

    def clear_all(self, run_time=0.8):
        for m in self.mobjects:
            m.clear_updaters()
        if self.mobjects:
            self.play(*[FadeOut(m) for m in self.mobjects], run_time=run_time)
        self.cap = None

    def chapter(self, num, title, sub=None):
        self.clear_all()
        n = T(f"Глава {num}", 30, GREY_B)
        t = T(title, 54)
        if t.width > 12.5:
            t.scale_to_fit_width(12.5)
        g = VGroup(n, t).arrange(DOWN, buff=0.35)
        line = Line(LEFT * 4, RIGHT * 4, color=C_A, stroke_width=2).next_to(g, DOWN, buff=0.35)
        items = [n, t, line]
        self.play(FadeIn(n, shift=0.2 * DOWN), Write(t), run_time=1.4)
        anims = [GrowFromCenter(line)]
        if sub:
            s = T(sub, 28, GREY_A).next_to(line, DOWN, buff=0.35)
            items.append(s)
            anims.append(FadeIn(s, shift=0.1 * UP))
        self.play(*anims)
        self.wait(1.6)
        self.play(*[FadeOut(m) for m in items])

    def derive(self, lhs, steps, size=36, buff=0.3, pos=ORIGIN, max_h=4.8, max_w=12.8):
        rows = [M(lhs, "=", steps[0][0], size=size)]
        for tex, _ in steps[1:]:
            rows.append(M("=", tex, size=size))
        g = VGroup(*rows).arrange(DOWN, buff=buff)
        for r in rows[1:]:
            r.shift((rows[0][1].get_left()[0] - r[0].get_left()[0]) * RIGHT)
        if g.height > max_h:
            g.scale_to_fit_height(max_h)
        if g.width > max_w:
            g.scale_to_fit_width(max_w)
        g.move_to(pos)
        if g.get_bottom()[1] < -2.55:
            g.shift(UP * (-2.55 - g.get_bottom()[1]))
        for i, (row, (_, cap)) in enumerate(zip(rows, steps)):
            if i == 0:
                self.play(Write(row), run_time=1.5)
            else:
                self.play(TransformMatchingShapes(rows[i - 1][-1].copy(), row), run_time=1.2)
            if cap:
                self.say(cap)
            else:
                self.wait(1.0)
        return g

    # ═══════════════════════ 0. Вступление ═══════════════════════════════
    def ch00_intro(self):
        title = T("Основная теорема анализа", 70)
        sub = T("почему площадь считается через первообразную", 34, GREY_A)
        VGroup(title, sub).arrange(DOWN, buff=0.45)
        self.play(Write(title), run_time=2.2)
        self.play(FadeIn(sub, shift=0.2 * UP))
        self.wait(2)
        self.play(FadeOut(title), FadeOut(sub))

        left = VGroup(T("путь s(t)", 38, C_A), T("положение в каждый момент", 24, GREY_B)).arrange(DOWN, buff=0.15)
        right = VGroup(T("скорость v(t)", 38, C_F), T("показания спидометра", 24, GREY_B)).arrange(DOWN, buff=0.15)
        left.move_to(LEFT * 4 + UP * 0.6)
        right.move_to(RIGHT * 4 + UP * 0.6)
        top = CurvedArrow(left.get_top() + UP * 0.1, right.get_top() + UP * 0.1, angle=-PI / 3, color=C_GOOD)
        top_l = M(r"\frac{d}{dt}", size=40, color=C_GOOD).next_to(top, UP, buff=0.1)
        bot = CurvedArrow(right.get_bottom() + DOWN * 0.1, left.get_bottom() + DOWN * 0.1, angle=-PI / 3, color=C_PINK)
        bot_l = M("?", size=56, color=C_PINK).next_to(bot, DOWN, buff=0.1)
        self.play(FadeIn(left), FadeIn(right))
        self.play(Create(top), Write(top_l))
        self.say("В прошлом ролике мы научились по пути находить скорость: "
                 "скорость — это производная пути, s'(t) = v(t).")
        self.play(Create(bot), Write(bot_l))
        self.say("Теперь обратная задача. Спидометр записывал скорость всю поездку. "
                 "Сколько километров мы проехали?")
        self.say("Оказывается, у этого вопроса есть геометрический ответ — площадь "
                 "под графиком скорости. А вычисляется она… через производную наоборот.")
        self.say("Связь между этими двумя идеями и называется Основной теоремой "
                 "анализа. Это, пожалуй, главная формула всего курса.")
        self.clear_all()

        head = T("Наш маршрут", 44, C_A).to_edge(UP, buff=0.5)
        items = ["путь = площадь", "суммы Римана", "определённый интеграл",
                 "функция площади A(x)", "теорема, часть 1: A' = f", "формула Ньютона–Лейбница",
                 "первообразные и примеры", "подводные камни", "итог"]
        boxes = VGroup()
        for i, s in enumerate(items):
            t = T(f"{i + 1}. {s}", 26)
            boxes.add(VGroup(SurroundingRectangle(t, buff=0.16, corner_radius=0.1, color=GREY_B,
                                                  stroke_width=1.5), t))
        boxes.arrange_in_grid(rows=3, cols=3, buff=(0.45, 0.6)).scale_to_fit_width(12.8)
        boxes.next_to(head, DOWN, buff=0.7)
        arrows = VGroup(*[Arrow(a.get_right(), b.get_left(), buff=0.06, stroke_width=3, color=GREY_B,
                                max_tip_length_to_length_ratio=0.3)
                          for a, b in zip(boxes[:-1], boxes[1:]) if abs(a.get_y() - b.get_y()) < 0.1])
        self.play(Write(head))
        self.play(LaggedStart(*[FadeIn(b, shift=0.2 * UP) for b in boxes], lag_ratio=0.25), run_time=3)
        self.play(LaggedStart(*[GrowArrow(a) for a in arrows], lag_ratio=0.2))
        self.wait(2)

    # ═══════════════════════ 1. Путь = площадь ═══════════════════════════
    def ch01_distance_area(self):
        self.chapter(1, "Путь = площадь", "самое простое наблюдение")
        ax = std_axes([0, 6.5, 1], [0, 120, 20], 7.2, 4.4).shift(LEFT * 2.4 + UP * 0.45)
        xl = ax.get_x_axis_label(M(r"t,\ \text{ч}", size=30), edge=RIGHT, direction=DR)
        yl = ax.get_y_axis_label(M(r"v,\ \text{км/ч}", size=28), edge=UP, direction=UP)
        self.play(Create(ax), Write(xl), Write(yl))

        const = ax.plot(lambda t: 60, x_range=[0, 3], color=C_F, stroke_width=4)
        rect = area_poly(ax, lambda t: 60, 0, 3, C_POS, 0.5)
        self.play(Create(const))
        self.say("Начнём с простого: 3 часа едем с постоянной скоростью 60 км/ч. "
                 "График скорости — горизонтальный отрезок.")
        self.play(FadeIn(rect))
        br1 = Brace(rect, DOWN, color=C_H)
        br2 = Brace(rect, LEFT, color=C_F)
        b1 = M(r"3\ \text{ч}", size=30, color=C_H).next_to(br1, DOWN, buff=0.1)
        b2 = M(r"60", size=30, color=C_F).next_to(br2, LEFT, buff=0.1)
        self.play(GrowFromCenter(br1), GrowFromCenter(br2), Write(b1), Write(b2))
        f1 = M(r"s = v\cdot t = 60\cdot 3 = 180\ \text{км}", size=38).to_edge(RIGHT, buff=0.4).shift(UP * 1.8)
        self.play(Write(f1))
        self.say("Путь = скорость × время. А это ровно площадь прямоугольника "
                 "под графиком: высота 60, ширина 3.")
        self.play(FadeOut(VGroup(const, rect, br1, br2, b1, b2)))

        speeds = [(0, 1, 40), (1, 2, 90), (2, 3.5, 60), (3.5, 5, 100), (5, 6, 30)]
        steps = VGroup(*[ax.plot(lambda t, v=v: v, x_range=[a, b], color=C_F, stroke_width=4)
                         for a, b, v in speeds])
        rects = VGroup(*[area_poly(ax, lambda t, v=v: v, a, b, C_POS, 0.5) for a, b, v in speeds])
        for r in rects:
            r.set_stroke(WHITE, 1)
        self.play(Create(steps), run_time=2)
        self.say("Теперь скорость меняется ступеньками. На каждом участке скорость "
                 "постоянна — значит, путь на участке равен площади прямоугольника.")
        self.play(LaggedStart(*[FadeIn(r) for r in rects], lag_ratio=0.3))
        f2 = M(r"s=\sum_k v_k\,\Delta t_k", size=40).next_to(f1, DOWN, buff=0.6)
        f2b = M(r"=40+90+90+150+30=400\ \text{км}", size=30).next_to(f2, DOWN, buff=0.3)
        f2b.scale_to_fit_width(min(f2b.width, 5.6)).set_x(4.1)
        self.play(Write(f2))
        self.play(Write(f2b))
        self.say("Весь путь — сумма площадей всех прямоугольников, то есть площадь "
                 "под ступенчатым графиком целиком.")
        self.play(FadeOut(VGroup(steps, rects, f2b)))

        v = lambda t: 20 + 70 * np.exp(-((t - 3) ** 2) / 3) + 4 * t
        smooth = ax.plot(v, x_range=[0, 6.3], color=C_F, stroke_width=4)
        self.play(Create(smooth), run_time=2)
        self.say("А если скорость меняется плавно, каждое мгновение? Прямоугольников "
                 "нет. Но идея та же: путь — это площадь под графиком скорости.")
        fill = area_poly(ax, v, 0, 6.3, C_POS, 0.5)
        self.play(FadeIn(fill))
        q = VGroup(T("Как посчитать", 32, C_A), T("площадь под кривой?", 32, C_A)).arrange(DOWN, buff=0.12)
        q.next_to(f2, DOWN, buff=0.7).set_x(4.4)
        self.play(Write(q))
        self.say("Площадь под кривой — вот задача, которую решают интегралы. "
                 "Первый шаг — приблизить кривую ступеньками.")

    # ═══════════════════════ 2. Суммы Римана ═════════════════════════════
    def ch02_riemann(self):
        self.chapter(2, "Суммы Римана", "режем площадь на полоски")
        f = lambda x: x ** 2
        ax = std_axes([0, 1, 0.25], [0, 1, 0.25], 5.8, 4.6, nums=False).shift(LEFT * 3.2 + UP * 0.35)
        ax.add_coordinates({0.5: M("0{,}5", size=22), 1: M("1", size=22)},
                           {0.5: M("0{,}5", size=22), 1: M("1", size=22)})
        g = ax.plot(f, x_range=[0, 1], color=C_F, stroke_width=4)
        gl = M("y=x^2", size=34, color=C_F).next_to(ax.c2p(0.55, 0.85), LEFT)
        self.play(Create(ax), Create(g), Write(gl))
        self.say("Пример: площадь под параболой y = x² от 0 до 1. Формулы площади "
                 "для такой фигуры в школе не было.")

        n = 4
        rl = riemann(ax, f, 0, 1, n, "left")
        self.play(LaggedStart(*[FadeIn(r) for r in rl], lag_ratio=0.2))
        self.say("Разрежем отрезок [0; 1] на n равных частей шириной Δx = 1/n. "
                 "Над каждой поставим прямоугольник высотой f в левом конце.")
        sums = {4: (0.21875, 0.46875), 8: (0.2734, 0.3984), 16: (0.3027, 0.3652),
                64: (0.3255, 0.3411), 1000: (0.33283, 0.33383)}
        form = M(r"S_n=\sum_{k=1}^{n} f(x_k)\,\Delta x", size=42).move_to(RIGHT * 3.4 + UP * 2.4)
        self.play(Write(form))
        head = VGroup(M("n", size=34), T("снизу", 26, C_F), T("сверху", 26, C_A))
        tbl_rows = [head]
        for k, (lo, hi) in sums.items():
            tbl_rows.append(VGroup(M(str(k), size=32), M(ru(lo, 4).replace(",", "{,}"), size=32, color=C_F),
                                   M(ru(hi, 4).replace(",", "{,}"), size=32, color=C_A)))
        grid = VGroup(*[c for row in tbl_rows for c in row]).arrange_in_grid(rows=6, cols=3, buff=(0.7, 0.26))
        grid.next_to(form, DOWN, buff=0.45)
        line = Line(grid.get_left(), grid.get_right(), color=GREY_B, stroke_width=1.5)
        line.next_to(VGroup(*head), DOWN, buff=0.12).set_x(grid.get_x())
        self.play(FadeIn(VGroup(*head)), Create(line))

        rr = riemann(ax, f, 0, 1, n, "right", color=C_A, opacity=0.25)
        self.play(FadeIn(rr))
        self.say("Левые прямоугольники дают площадь с недостатком, правые (жёлтые) — "
                 "с избытком. Настоящая площадь зажата между ними.")
        self.play(FadeIn(VGroup(*tbl_rows[1])))
        for i, k in enumerate([8, 16, 64], start=2):
            nl, nr = riemann(ax, f, 0, 1, k, "left"), riemann(ax, f, 0, 1, k, "right", color=C_A, opacity=0.25)
            self.play(ReplacementTransform(rl, nl), ReplacementTransform(rr, nr), run_time=1.3)
            rl, rr = nl, nr
            self.play(FadeIn(VGroup(*tbl_rows[i])), run_time=0.6)
            self.wait(0.6)
        self.play(FadeIn(VGroup(*tbl_rows[5])))
        self.say("Чем больше n, тем тоньше полоски и тем ближе нижняя и верхняя суммы. "
                 "Обе подбираются к 0,3333… = 1/3.")
        self.clear_all()

        head = T("Считаем точно — через предел", 36, C_A).to_edge(UP, buff=0.4)
        self.play(Write(head))
        self.derive(
            "S_n",
            [(r"\sum_{k=1}^{n}\left(\frac{k}{n}\right)^2\cdot\frac1n",
              "Правые концы: xₖ = k/n, высота (k/n)², ширина 1/n."),
             (r"\frac{1}{n^3}\sum_{k=1}^{n}k^2", "Выносим 1/n³ за знак суммы."),
             (r"\frac{1}{n^3}\cdot\frac{n(n+1)(2n+1)}{6}",
              "Сумма квадратов 1² + 2² + … + n² — известная формула (доказывается индукцией)."),
             (r"\frac{1}{6}\left(1+\frac1n\right)\left(2+\frac1n\right)\ \xrightarrow[n\to\infty]{}\ \frac{2}{6}=\frac13",
              "При n → ∞ дроби 1/n исчезают. Площадь равна ровно 1/3.")],
            size=36, pos=DOWN * 0.1)
        self.say("Ответ получили, но ценой усилий: понадобилась хитрая формула суммы. "
                 "Для sin x или eˣ так мучиться не хочется. Запомним число 1/3.")

    # ═══════════════════════ 3. Определённый интеграл ════════════════════
    def ch03_definite_integral(self):
        self.chapter(3, "Определённый интеграл", "предел сумм Римана")
        d = M(r"\int_a^b f(x)\,dx", "=", r"\lim_{n\to\infty}", r"\sum_{k=1}^{n} f(x_k^*)\,\Delta x", size=54)
        d[0].set_color(C_A)
        d[2].set_color(C_LIM)
        d.move_to(UP * 2.55)
        self.play(Write(d), run_time=2.5)
        self.say("Определённый интеграл — это предел сумм Римана, когда полоски "
                 "становятся бесконечно тонкими. Точка xₖ* в полоске — любая.")
        notes = VGroup(
            VGroup(M(r"\int", size=56, color=C_A), T("вытянутая S — «сумма» (Лейбниц)", 26)).arrange(RIGHT, buff=0.4),
            VGroup(M(r"f(x)", size=44), T("высота полоски", 26)).arrange(RIGHT, buff=0.4),
            VGroup(M(r"dx", size=44, color=C_H), T("её ширина — память о Δx → 0", 26)).arrange(RIGHT, buff=0.4),
            VGroup(M(r"a,\ b", size=44), T("пределы интегрирования: откуда и докуда", 26)).arrange(RIGHT, buff=0.4),
        ).arrange(DOWN, aligned_edge=LEFT, buff=0.22).next_to(d, DOWN, buff=0.4)
        for n_ in notes:
            self.play(FadeIn(n_, shift=RIGHT * 0.3), run_time=0.7)
            self.wait(1.2)
        self.say("Обозначение буквально читается: «сложить произведения f(x)·dx "
                 "от a до b». Интеграл — это сумма бесконечно тонких полосок.")
        self.say("Для непрерывной функции этот предел всегда существует и не зависит "
                 "от выбора точек xₖ*. Такие функции называются интегрируемыми.")
        self.clear_all()

        # знак площади
        f = lambda x: np.sin(x)
        ax = std_axes([0, 2 * PI, PI / 2], [-1.3, 1.3, 1], 8, 3.6, nums=False).shift(UP * 0.8)
        ax.add_coordinates({PI: M(r"\pi", size=28), 2 * PI: M(r"2\pi", size=28)}, {1: M("1", size=22), -1: M("-1", size=22)})
        g = ax.plot(f, x_range=[0, 2 * PI], color=C_F, stroke_width=4)
        self.play(Create(ax), Create(g))
        ar = signed_area(ax, f, 0, 2 * PI, 0.5)
        self.play(FadeIn(ar))
        pl = M("+", size=60, color=C_POS).move_to(ax.c2p(PI / 2, 0.45))
        mi = M("-", size=60, color=C_NEG).move_to(ax.c2p(3 * PI / 2, -0.45))
        self.play(Write(pl), Write(mi))
        res = M(r"\int_0^{2\pi}\sin x\,dx = 0", size=44).next_to(ax, DOWN, buff=0.35)
        self.play(Write(res))
        self.say("Важно: интеграл считает площадь СО ЗНАКОМ. Там, где f < 0, высоты "
                 "полосок отрицательны, и площадь вычитается. У синуса части гасят друг друга.")
        self.say("Физически: отрицательная скорость — едем назад. Интеграл скорости — "
                 "перемещение, а не пройденный путь.")
        self.clear_all()

        head = T("Свойства (все — из свойств сумм и пределов)", 34, C_A).to_edge(UP, buff=0.4)
        props = VGroup(
            M(r"\int_a^b \big(\alpha f+\beta g\big)\,dx=\alpha\int_a^b f\,dx+\beta\int_a^b g\,dx", size=36),
            M(r"\int_a^b f\,dx+\int_b^c f\,dx=\int_a^c f\,dx", size=36),
            M(r"\int_a^a f\,dx=0,\qquad \int_b^a f\,dx=-\int_a^b f\,dx", size=36),
            M(r"m\le f(x)\le M\ \text{на}\ [a;b]\ \Rightarrow\ m(b-a)\le\int_a^b f\,dx\le M(b-a)", size=36),
        ).arrange(DOWN, buff=0.34).next_to(head, DOWN, buff=0.4)
        caps = ["Линейность: сумму и множитель можно выносить — как у производной.",
                "Аддитивность: площадь от a до c = площадь от a до b + от b до c.",
                "Договорённости: нулевой отрезок даёт 0, смена направления меняет знак.",
                "Оценка: площадь зажата между прямоугольниками высоты min и max. "
                "Это свойство станет ключом к доказательству теоремы."]
        self.play(Write(head))
        for p, c in zip(props, caps):
            self.play(Write(p), run_time=1.3)
            self.say(c)
        self.play(Circumscribe(props[3], color=C_A))
        self.wait(1)

    # ═══════════════════════ 4. Функция площади ══════════════════════════
    def ch04_area_function(self):
        self.chapter(4, "Функция площади", "двигаем правый край")
        f = lambda x: 1.5 * np.sin(x) + 0.3
        A = lambda x: 1.5 * (1 - np.cos(x)) + 0.3 * x
        top = std_axes([0, 6.5, 1], [-1.5, 2, 1], 9, 2.6, font=20).move_to(UP * 1.75)
        bot = std_axes([0, 6.5, 1], [0, 5, 1], 9, 2.3, font=20).move_to(DOWN * 1.35)
        tl = M("f(t)", size=32, color=C_F).next_to(top, LEFT, buff=0.3)
        bl = M("A(x)", size=32, color=C_A).next_to(bot, LEFT, buff=0.3)
        g = top.plot(f, x_range=[0, 6.5], color=C_F, stroke_width=4)
        self.play(Create(top), Create(g), Write(tl))
        self.say("Возьмём функцию f и зафиксируем левый край a = 0. Правый край x "
                 "сделаем подвижным.")
        x = ValueTracker(0.01)
        ar = always_redraw(lambda: signed_area(top, f, 0, x.get_value(), 0.5))
        edge = always_redraw(lambda: Line(top.c2p(x.get_value(), 0), top.c2p(x.get_value(), f(x.get_value())),
                                          color=C_A, stroke_width=4))
        xlab = always_redraw(lambda: M("x", size=30, color=C_A).next_to(top.c2p(x.get_value(), 0), DOWN, buff=0.35))
        self.add(ar)
        self.play(FadeIn(edge), FadeIn(xlab))
        self.play(x.animate.set_value(2), run_time=2.5)
        df = M(r"A(x)=\int_0^x f(t)\,dt", size=38, color=C_A).to_corner(UR, buff=0.4).shift(DOWN * 0.1)
        self.play(Write(df))
        self.say("Площадь от 0 до x — это число, которое зависит от x. Получилась "
                 "новая функция A(x). Буква t внутри — просто переменная интегрирования.")
        self.play(Create(bot), Write(bl))
        tr = always_redraw(lambda: bot.plot(A, x_range=[0.01, max(0.02, x.get_value())], color=C_A, stroke_width=4))
        bd = always_redraw(lambda: Dot(bot.c2p(x.get_value(), A(x.get_value())), color=C_A, radius=0.06))
        vl = always_redraw(lambda: DashedLine(top.c2p(x.get_value(), 0), bot.c2p(x.get_value(), A(x.get_value())),
                                              color=GREY_C, stroke_width=1.5))
        self.play(FadeIn(tr), FadeIn(bd), FadeIn(vl))
        self.play(x.animate.set_value(PI), run_time=3, rate_func=linear)
        self.say("Пока f большая, площадь растёт быстро: график A круто идёт вверх. "
                 "Где f маленькая — A растёт медленно.")
        self.play(x.animate.set_value(3.34), run_time=1.2, rate_func=linear)
        self.say("Вот f обнулилась — и A на мгновение перестала расти: её касательная "
                 "горизонтальна. Это вершина графика A.")
        self.play(x.animate.set_value(6.08), run_time=3, rate_func=linear)
        self.say("Где f < 0, добавляется отрицательная площадь, и A убывает. "
                 "Когда f снова становится положительной, A опять растёт.")
        self.play(x.animate.set_value(6.45), run_time=1, rate_func=linear)
        guess = M(r"A'(x)\ \overset{?}{=}\ f(x)", size=48, color=C_A).move_to(UP * 0.2 + RIGHT * 3)
        box = SurroundingRectangle(guess, color=C_A, buff=0.2).set_fill(C_BG, 0.9)
        self.play(FadeIn(box), Write(guess))
        self.say("Знак f управляет ростом A, величина f — скоростью роста. Очень похоже, "
                 "что скорость роста площади — это сама функция: A' = f. Докажем!")

    # ═══════════════════════ 5. ОТА, часть 1 ═════════════════════════════
    def ch05_ftc1(self):
        self.chapter(5, "Теорема, часть 1", "скорость роста площади равна f")
        f = lambda x: 0.35 * (x - 1) ** 2 + 0.6 + 0.25 * np.sin(2 * x)
        ax = std_axes([0, 4, 1], [0, 3.5, 1], 6.4, 4.3, nums=False).shift(LEFT * 3.1 + UP * 0.4)
        g = ax.plot(f, x_range=[0, 3.75], color=C_F, stroke_width=4)
        self.play(Create(ax), Create(g))
        x0 = 2.0
        base = signed_area(ax, f, 0.2, x0, 0.35)
        h = ValueTracker(0.9)
        strip = always_redraw(lambda: area_poly(ax, f, x0, x0 + h.get_value(), C_A, 0.7))
        self.play(FadeIn(base))
        self.play(FadeIn(strip))
        hl = always_redraw(lambda: BraceBetweenPoints(ax.c2p(x0, 0), ax.c2p(x0 + h.get_value(), 0),
                                                      DOWN, color=C_H))
        hlt = always_redraw(lambda: M("h", size=30, color=C_H).next_to(hl, DOWN, buff=0.08))
        xlab = M("x", size=30).next_to(ax.c2p(x0, 0), DOWN, buff=0.5).shift(LEFT * 0.25)
        self.play(FadeIn(hl), FadeIn(hlt), Write(xlab))
        s1 = M(r"A(x+h)-A(x)", "=", r"\int_x^{x+h} f(t)\,dt", size=36).move_to(RIGHT * 3.4 + UP * 2.5)
        self.play(Write(s1))
        self.say("Сдвинем правый край с x на x + h. Площадь прибавилась на жёлтую "
                 "полоску — по свойству аддитивности это интеграл от x до x + h.")

        def minmax():
            xs = np.linspace(x0, x0 + h.get_value(), 60)
            ys = f(xs)
            return ys.min(), ys.max()

        low = always_redraw(lambda: Rectangle(
            width=ax.c2p(x0 + h.get_value(), 0)[0] - ax.c2p(x0, 0)[0],
            height=ax.c2p(0, minmax()[0])[1] - ax.c2p(0, 0)[1]).set_stroke(C_H, 3).set_fill(opacity=0)
            .align_to(ax.c2p(x0, 0), DL))
        high = always_redraw(lambda: Rectangle(
            width=ax.c2p(x0 + h.get_value(), 0)[0] - ax.c2p(x0, 0)[0],
            height=ax.c2p(0, minmax()[1])[1] - ax.c2p(0, 0)[1]).set_stroke(C_PINK, 3).set_fill(opacity=0)
            .align_to(ax.c2p(x0, 0), DL))
        self.play(Create(low), Create(high))
        s2 = M(r"m_h\cdot h", r"\le", r"A(x+h)-A(x)", r"\le", r"M_h\cdot h", size=36).next_to(s1, DOWN, buff=0.5)
        s2[0].set_color(C_H)
        s2[4].set_color(C_PINK)
        self.play(Write(s2))
        self.say("Пусть mₕ и Mₕ — наименьшее и наибольшее значения f на [x; x+h]. "
                 "Полоска лежит между двумя прямоугольниками: mₕ·h ≤ ΔA ≤ Mₕ·h.")
        s3 = M(r"m_h", r"\le", r"\frac{A(x+h)-A(x)}{h}", r"\le", r"M_h", size=36).next_to(s2, DOWN, buff=0.5)
        s3[0].set_color(C_H)
        s3[4].set_color(C_PINK)
        self.play(Write(s3))
        self.say("Делим на h > 0 — посередине оказалось разностное отношение для A. "
                 "Узнаёте? Это наклон секущей графика A!")
        self.play(h.animate.set_value(0.08), run_time=4, rate_func=rate_functions.ease_in_out_sine)
        s4 = M(r"m_h\to f(x),\quad M_h\to f(x)\quad (h\to 0)", size=34).next_to(s3, DOWN, buff=0.5)
        self.play(Write(s4))
        self.say("Когда h → 0, отрезок стягивается в точку x. Так как f непрерывна, "
                 "и минимум, и максимум стремятся к f(x). Нижний и верхний прямоугольники сливаются.")
        s5 = M(r"A'(x)=f(x)", size=48, color=C_A).next_to(s4, DOWN, buff=0.5)
        self.play(Write(s5))
        self.play(Circumscribe(s5, color=C_A))
        self.say("Разностное отношение зажато между двумя величинами, стремящимися "
                 "к f(x). По теореме о двух милиционерах его предел тоже f(x).")
        self.clear_all()

        thm = VGroup(
            T("Основная теорема анализа, часть 1", 36, C_A),
            M(r"f\ \text{непрерывна на}\ [a;b]\ \Longrightarrow\ \frac{d}{dx}\int_a^x f(t)\,dt=f(x)", size=44),
        ).arrange(DOWN, buff=0.6).move_to(UP * 0.8)
        frame = SurroundingRectangle(thm, color=C_A, buff=0.4, corner_radius=0.15)
        self.play(Write(thm[0]), Write(thm[1]), run_time=2)
        self.play(Create(frame))
        self.say("Сначала интегрируем, потом дифференцируем — и возвращаемся к "
                 "исходной функции. Интегрирование и дифференцирование взаимно обратны.")
        self.say("Отсюда важное следствие: у любой непрерывной функции есть первообразная — "
                 "функция, производная которой равна f. Это A(x).")

    # ═══════════════════════ 6. Ньютон–Лейбниц ═══════════════════════════
    def ch06_newton_leibniz(self):
        self.chapter(6, "Формула Ньютона–Лейбница", "часть 2: как считать интегралы")
        self.say("Первообразная функции f — любая функция F с F' = f. Например, "
                 "для f = x² подходит F = x³/3, ведь (x³/3)' = x².")
        head = T("Доказательство", 36, C_A).to_edge(UP, buff=0.4)
        self.play(Write(head))
        steps = VGroup(
            M(r"A(x)=\int_a^x f(t)\,dt,\qquad A'=f\ \ (\text{часть 1})", size=36),
            M(r"F'=f\ \Rightarrow\ (F-A)'=f-f=0", size=36),
            M(r"\Rightarrow\ F(x)-A(x)=C\ \ \text{(константа)}", size=36),
            M(r"x=a:\ \ A(a)=0\ \Rightarrow\ C=F(a)", size=36),
            M(r"x=b:\ \ \int_a^b f(t)\,dt=A(b)=F(b)-F(a)", size=40, color=C_A),
        ).arrange(DOWN, aligned_edge=LEFT, buff=0.4).next_to(head, DOWN, buff=0.5)
        caps = ["Функция площади A — одна первообразная. Пусть F — любая другая.",
                "Разность двух первообразных имеет нулевую производную…",
                "…а функция с нулевой производной на отрезке — константа "
                "(это следствие теоремы Лагранжа о среднем).",
                "В точке a площадь нулевая, значит C = F(a), и A(x) = F(x) − F(a).",
                "Подставляем x = b. Площадь равна разности значений ЛЮБОЙ первообразной!"]
        for s, c in zip(steps, caps):
            self.play(Write(s), run_time=1.2)
            self.say(c)
        self.clear_all()

        nl = M(r"\int_a^b f(x)\,dx", "=", r"F(b)-F(a)", "=", r"F(x)\Big|_a^b", size=60)
        nl[0].set_color(C_F)
        nl[2].set_color(C_A)
        nl.move_to(UP * 1.2)
        self.play(Write(nl), run_time=2.5)
        frame = SurroundingRectangle(nl, color=C_A, buff=0.3, corner_radius=0.15)
        self.play(Create(frame))
        self.say("Формула Ньютона–Лейбница. Чтобы найти площадь, не нужно "
                 "суммировать полоски: достаточно знать первообразную на двух концах.")
        chk = M(r"\int_0^1 x^2\,dx=\frac{x^3}{3}\Big|_0^1=\frac13-0=\frac13", size=46, color=C_GOOD).next_to(frame, DOWN, buff=0.7)
        self.play(Write(chk))
        self.say("Проверим на параболе: x³/3 от 0 до 1 даёт 1/3 — в одну строчку. "
                 "А через суммы Римана мы добывали это число целую главу.")
        self.clear_all()

        # интуиция: телескопическая сумма
        F = lambda x: x ** 3 / 3
        f = lambda x: x ** 2
        a, b, n = 0.4, 1.9, 6
        dx = (b - a) / n
        top = std_axes([0, 2, 0.5], [0, 2.5, 1], 7.5, 2.6, nums=False).move_to(UP * 1.95 + LEFT * 1.8)
        bot = std_axes([0, 2, 0.5], [0, 4, 1], 7.5, 2.0, nums=False).move_to(DOWN * 0.95 + LEFT * 1.8)
        tl = M("F", size=34, color=C_A).next_to(top, LEFT, buff=0.3)
        bl = M("f=F'", size=34, color=C_F).next_to(bot, LEFT, buff=0.3)
        Fg = top.plot(F, x_range=[0, 2], color=C_A, stroke_width=4)
        fg = bot.plot(f, x_range=[0, 2], color=C_F, stroke_width=4)
        self.play(Create(top), Create(bot), Create(Fg), Create(fg), Write(tl), Write(bl))
        self.say("Есть и наглядный смысл. Разобьём [a; b] на кусочки и посмотрим, "
                 "на сколько меняется F на каждом из них.")
        steps_F = VGroup()
        cols = color_gradient([C_H, C_A, C_PINK], n)
        for k in range(n):
            x0, x1 = a + k * dx, a + (k + 1) * dx
            steps_F.add(VGroup(
                Line(top.c2p(x0, F(x0)), top.c2p(x1, F(x0)), color=GREY_B, stroke_width=2),
                Line(top.c2p(x1, F(x0)), top.c2p(x1, F(x1)), color=cols[k], stroke_width=6)))
        self.play(LaggedStart(*[Create(s) for s in steps_F], lag_ratio=0.25), run_time=3)
        col = VGroup(*[s[1].copy() for s in steps_F])
        target_x = top.c2p(2.35, 0)[0]
        stack = VGroup()
        y = top.c2p(0, F(a))[1]
        for s in col:
            h_ = s.get_length()
            stack.add(Line([target_x, y, 0], [target_x, y + h_, 0], color=s.get_color(), stroke_width=8))
            y += h_
        self.play(*[Transform(c, t) for c, t in zip(col, stack)], run_time=2)
        tot = M(r"F(b)-F(a)=\sum_k \Delta F_k", size=32, color=C_A).next_to(stack, RIGHT, buff=0.3)
        self.play(Write(tot))
        self.say("Все ступеньки ΔFₖ вместе складываются в полное изменение "
                 "F(b) − F(a). Сумма «телескопическая»: промежуточные значения сокращаются.")
        rects = riemann(bot, f, a, b, n, "left", opacity=0.55)
        for r, c in zip(rects, cols):
            r.set_fill(c, 0.6)
        self.play(LaggedStart(*[FadeIn(r) for r in rects], lag_ratio=0.2))
        approx = M(r"\Delta F_k\approx F'(x_k)\,\Delta x=f(x_k)\,\Delta x", size=32).next_to(bot, RIGHT, buff=0.2).shift(UP * 0.4)
        if approx.get_right()[0] > 7:
            approx.scale_to_fit_width(7 - bot.get_right()[0] - 0.3).next_to(bot, RIGHT, buff=0.15).shift(UP * 0.4)
        self.play(Write(approx))
        self.say("Но каждая ступенька ΔFₖ ≈ F'(xₖ)·Δx = f(xₖ)·Δx — это площадь "
                 "прямоугольника того же цвета внизу! Линейное приближение из прошлого ролика.")
        self.say("Значит, F(b) − F(a) ≈ сумма Римана для f. При Δx → 0 ошибка исчезает, "
                 "и приближённое равенство становится точным. Вот и вся теорема.")

    # ═══════════════════════ 7. Первообразные ════════════════════════════
    def ch07_antiderivatives(self):
        self.chapter(7, "Первообразные и примеры", "таблица производных — наоборот")
        ax = std_axes([-2, 2, 1], [-2, 4, 1], 6, 4.4).shift(LEFT * 3.3 + UP * 0.35)
        C = ValueTracker(0)
        fam = VGroup(*[ax.plot(lambda x, c=c: x ** 3 / 3 + c, x_range=[-1.9, 1.9], color=C_A,
                               stroke_width=2, stroke_opacity=0.35) for c in (-1.5, -0.5, 0.5, 1.5, 2.5)])
        main = always_redraw(lambda: ax.plot(lambda x: x ** 3 / 3 + C.get_value(), x_range=[-1.9, 1.9],
                                             color=C_A, stroke_width=4))
        tg = always_redraw(lambda: Line(ax.c2p(0.4, 1 / 3 + C.get_value() - 1),
                                        ax.c2p(1.6, 1 / 3 + C.get_value() + 1), color=C_F, stroke_width=3))
        self.play(Create(ax), FadeIn(fam), FadeIn(main), FadeIn(tg))
        lab = M(r"F(x)=\frac{x^3}{3}+C", size=40, color=C_A).move_to(RIGHT * 3.4 + UP * 2.4)
        self.play(Write(lab))
        self.play(C.animate.set_value(2), run_time=2)
        self.play(C.animate.set_value(-1), run_time=2)
        self.say("Первообразная определена не однозначно: прибавим константу — производная "
                 "не изменится. Сдвинутые вверх-вниз графики имеют одинаковые наклоны.")
        ind = M(r"\int f(x)\,dx=F(x)+C", size=40).next_to(lab, DOWN, buff=0.5)
        self.play(Write(ind))
        self.say("Всё семейство записывают как неопределённый интеграл. В формуле "
                 "Ньютона–Лейбница константа сокращается: (F(b)+C) − (F(a)+C).")
        self.clear_all()

        head = T("Таблица: читаем производные справа налево", 34, C_A).to_edge(UP, buff=0.35)
        rows = [
            (r"x^n\ (n\ne-1)", r"\frac{x^{n+1}}{n+1}"),
            (r"\frac1x", r"\ln|x|"),
            (r"e^x", r"e^x"),
            (r"\cos x", r"\sin x"),
            (r"\sin x", r"-\cos x"),
        ]
        cells = [T("f(x)", 28, C_F), T("первообразная F(x)", 28, C_A)]
        for fx, Fx in rows:
            cells += [M(fx, size=38, color=C_F), M(Fx, size=38, color=C_A)]
        grid = VGroup(*cells).arrange_in_grid(rows=6, cols=2, buff=(2.0, 0.32)).next_to(head, DOWN, buff=0.45)
        ln = Line(grid.get_left() + LEFT * 0.3, grid.get_right() + RIGHT * 0.3, color=GREY_B, stroke_width=1.5)
        ln.next_to(VGroup(*cells[:2]), DOWN, buff=0.14).set_x(grid.get_x())
        self.play(Write(head), FadeIn(VGroup(*cells[:2])), Create(ln))
        for i in range(len(rows)):
            self.play(FadeIn(cells[2 + 2 * i]), FadeIn(cells[3 + 2 * i], shift=LEFT * 0.3), run_time=0.8)
        self.say("Каждая строчка проверяется дифференцированием правого столбца. "
                 "Особый случай n = −1: формула степени ломается, и на помощь приходит ln|x|.")
        self.clear_all()

        # примеры
        f = np.sin
        ax = std_axes([0, PI, PI / 2], [0, 1.3, 1], 5.6, 3.2, nums=False).shift(LEFT * 3.4 + UP * 1.0)
        ax.add_coordinates({PI / 2: M(r"\frac{\pi}{2}", size=26), PI: M(r"\pi", size=26)}, {1: M("1", size=22)})
        g = ax.plot(f, x_range=[0, PI], color=C_F, stroke_width=4)
        ar = area_poly(ax, f, 0, PI, C_POS, 0.5)
        self.play(Create(ax), Create(g))
        self.play(FadeIn(ar))
        e1 = M(r"\int_0^{\pi}\sin x\,dx=-\cos x\Big|_0^{\pi}=-(-1)-(-1)=2", size=38).move_to(RIGHT * 2.6 + UP * 1.5)
        e1.scale_to_fit_width(min(e1.width, 6.3)).set_x(3.6)
        self.play(Write(e1))
        self.say("Площадь под одной аркой синуса — ровно 2. Красивое целое число "
                 "из кривой, на вид никак не связанной с целыми числами.")
        e2 = M(r"\int_1^{e}\frac{dx}{x}=\ln x\Big|_1^{e}=1-0=1", size=38).next_to(e1, DOWN, buff=0.5).set_x(3.6)
        self.play(Write(e2))
        self.say("Площадь под гиперболой 1/x от 1 до e равна 1. Это можно считать "
                 "ещё одним определением числа e.")
        e3 = M(r"s=\int_0^{2}9{,}8\,t\,dt=4{,}9\,t^2\Big|_0^{2}=19{,}6\ \text{м}", size=38).next_to(e2, DOWN, buff=0.5)
        e3.scale_to_fit_width(min(e3.width, 6.3)).set_x(3.6)
        self.play(Write(e3))
        self.say("И возвращаемся к вопросу из начала: камень падает со скоростью 9,8t. "
                 "За 2 секунды он пролетает 19,6 м — ответ прямо из записи скорости.")

    # ═══════════════════════ 8. Подводные камни ══════════════════════════
    def ch08_pitfalls(self):
        self.chapter(8, "Подводные камни", "когда теорема не работает")
        f = lambda x: 1 / x ** 2
        ax = std_axes([-1.2, 1.2, 1], [0, 8, 2], 6, 4.4).shift(LEFT * 3.2 + UP * 0.35)
        g1 = ax.plot(f, x_range=[-1.2, -0.36], color=C_F, stroke_width=4)
        g2 = ax.plot(f, x_range=[0.36, 1.2], color=C_F, stroke_width=4)
        a1 = area_poly(ax, lambda x: min(f(x), 8), -1, -0.354, C_POS, 0.45)
        a2 = area_poly(ax, lambda x: min(f(x), 8), 0.354, 1, C_POS, 0.45)
        self.play(Create(ax), Create(g1), Create(g2))
        self.play(FadeIn(a1), FadeIn(a2))
        wrong = M(r"\int_{-1}^{1}\frac{dx}{x^2}=-\frac1x\Big|_{-1}^{1}=-1-1=-2\ \ ???", size=38)
        wrong.scale_to_fit_width(min(wrong.width, 6.2)).move_to(RIGHT * 3.6 + UP * 1.8)
        self.play(Write(wrong))
        self.say("Посчитаем «по формуле»: первообразная −1/x, получаем −2. Но функция "
                 "1/x² положительна! Площадь под ней не может быть отрицательной.")
        cross = Cross(wrong, stroke_color=C_BAD, stroke_width=5)
        self.play(Create(cross))
        why = VGroup(T("Ошибка: f разрывна в 0 (уходит в ∞),", 28, C_BAD),
                     T("а теорема требует непрерывности на [a; b].", 28, C_BAD)).arrange(DOWN, buff=0.15)
        why.scale_to_fit_width(min(why.width, 6.2)).next_to(wrong, DOWN, buff=0.6).set_x(3.6)
        self.play(FadeIn(why))
        self.say("Теорема требует непрерывности на всём отрезке, а здесь в нуле "
                 "бесконечный разрыв. На самом деле площадь бесконечна — это «несобственный интеграл».")
        self.clear_all()

        head = T("Скачок: площадь есть, производной у A нет", 34, C_A).to_edge(UP, buff=0.4)
        self.play(Write(head))
        top = std_axes([0, 4, 1], [0, 2.5, 1], 7, 2.2, font=20).move_to(UP * 1.45)
        bot = std_axes([0, 4, 1], [0, 5, 1], 7, 2.2, font=20).move_to(DOWN * 1.35)
        f = lambda x: 0.6 if x < 2 else 1.8
        A = lambda x: 0.6 * x if x < 2 else 1.2 + 1.8 * (x - 2)
        s1 = top.plot(lambda x: 0.6, x_range=[0, 2], color=C_F, stroke_width=4)
        s2 = top.plot(lambda x: 1.8, x_range=[2, 4], color=C_F, stroke_width=4)
        self.play(Create(top), Create(bot), Create(s1), Create(s2))
        x = ValueTracker(0.01)
        ar = always_redraw(lambda: area_poly(top, f, 0, x.get_value(), C_POS, 0.5, n=300))
        tr = always_redraw(lambda: bot.plot(A, x_range=[0.01, max(0.02, x.get_value())], color=C_A, stroke_width=4))
        self.add(ar, tr)
        self.play(x.animate.set_value(3.95), run_time=4, rate_func=linear)
        corner = Circle(radius=0.25, color=C_BAD, stroke_width=3).move_to(bot.c2p(2, 1.2))
        self.play(Create(corner))
        self.say("У ступенчатой функции площадь считается без проблем, и A(x) непрерывна. "
                 "Но в точке скачка у графика A излом: A' там не существует.")
        self.say("Вывод: условие непрерывности f — не формальность. Там, где f рвётся, "
                 "рвётся и связь A' = f.")

    # ═══════════════════════ 9. Итог ═════════════════════════════════════
    def ch09_summary(self):
        self.chapter(9, "Итог", "две стороны одной медали")
        F = VGroup(T("F(x)", 44, C_A), T("первообразная / путь", 24, GREY_B)).arrange(DOWN, buff=0.15).move_to(LEFT * 4 + DOWN * 0.1)
        f = VGroup(T("f(x)", 44, C_F), T("функция / скорость", 24, GREY_B)).arrange(DOWN, buff=0.15).move_to(RIGHT * 4 + DOWN * 0.1)
        a1 = CurvedArrow(F.get_top() + UP * 0.1, f.get_top() + UP * 0.1, angle=-PI / 3, color=C_GOOD)
        a1l = M(r"\frac{d}{dx}", size=40, color=C_GOOD).next_to(a1, UP, buff=0.1)
        a2 = CurvedArrow(f.get_bottom() + DOWN * 0.1, F.get_bottom() + DOWN * 0.1, angle=-PI / 3, color=C_PINK)
        a2l = M(r"\int", size=48, color=C_PINK).next_to(a2, DOWN, buff=0.1)
        self.play(FadeIn(F), FadeIn(f))
        self.play(Create(a1), Write(a1l), Create(a2), Write(a2l), run_time=2)
        self.say("Производная превращает путь в скорость, интеграл — скорость в путь. "
                 "Две операции, обратные друг другу.")
        self.clear_all()

        eqs = VGroup(
            T("Часть 1", 30, GREY_A),
            M(r"\frac{d}{dx}\int_a^x f(t)\,dt=f(x)", size=48, color=C_A),
            T("Часть 2 (Ньютон–Лейбниц)", 30, GREY_A),
            M(r"\int_a^b F'(x)\,dx=F(b)-F(a)", size=48, color=C_A),
        ).arrange(DOWN, buff=0.4).move_to(UP * 0.6)
        self.play(LaggedStart(*[Write(e) for e in eqs], lag_ratio=0.4), run_time=4)
        self.play(Circumscribe(eqs[3], color=C_A))
        self.say("Часть 1: скорость роста площади равна высоте графика. Часть 2: "
                 "сумма всех маленьких изменений равна полному изменению.")
        self.say("Площадь, путь, работа, объём, вероятность — всё, что «накапливается», "
                 "считается через первообразную. В этом сила теоремы.")
        self.clear_all()
        thanks = T("Спасибо за внимание!", 56)
        sub = T("Дальше: техники интегрирования и ряды Тейлора", 30, GREY_A).next_to(thanks, DOWN, buff=0.4)
        self.play(Write(thanks))
        self.play(FadeIn(sub, shift=UP * 0.2))
        self.wait(3)
        self.play(FadeOut(thanks), FadeOut(sub))


CHAPTERS = [
    "ch00_intro", "ch01_distance_area", "ch02_riemann", "ch03_definite_integral",
    "ch04_area_function", "ch05_ftc1", "ch06_newton_leibniz", "ch07_antiderivatives",
    "ch08_pitfalls", "ch09_summary",
]


class FullVideo(Story):
    """Весь фильм одним роликом."""

    def construct(self):
        for name in CHAPTERS:
            getattr(self, name)()


def _chapter_scene(name, method):
    return type(name, (Story,), {"construct": lambda self: getattr(self, method)()})


Ch00_Intro = _chapter_scene("Ch00_Intro", "ch00_intro")
Ch01_DistanceArea = _chapter_scene("Ch01_DistanceArea", "ch01_distance_area")
Ch02_Riemann = _chapter_scene("Ch02_Riemann", "ch02_riemann")
Ch03_DefiniteIntegral = _chapter_scene("Ch03_DefiniteIntegral", "ch03_definite_integral")
Ch04_AreaFunction = _chapter_scene("Ch04_AreaFunction", "ch04_area_function")
Ch05_FTC1 = _chapter_scene("Ch05_FTC1", "ch05_ftc1")
Ch06_NewtonLeibniz = _chapter_scene("Ch06_NewtonLeibniz", "ch06_newton_leibniz")
Ch07_Antiderivatives = _chapter_scene("Ch07_Antiderivatives", "ch07_antiderivatives")
Ch08_Pitfalls = _chapter_scene("Ch08_Pitfalls", "ch08_pitfalls")
Ch09_Summary = _chapter_scene("Ch09_Summary", "ch09_summary")
