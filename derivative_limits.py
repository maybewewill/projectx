"""
Производная через пределы — видео на Manim Community (стиль 3Blue1Brown).

Рендер всего фильма одним роликом:
    manim -qh derivative_limits.py FullVideo
Отдельная глава (например, ε–δ):
    manim -qm derivative_limits.py Ch05_EpsilonDelta

Требования: manim >= 0.18, LaTeX с кириллицей (texlive-lang-cyrillic, cm-super),
шрифт «CMU Serif» (fonts-cmu) — при его отсутствии Pango подставит другой.

──────────────────────────────────────────────────────────────────────────────
ПЛАН (по методике manim-composer)
──────────────────────────────────────────────────────────────────────────────
Вопрос-крючок : спидометр показывает «2 м/с в момент t = 1». Но скорость — это
                путь/время, а за одно мгновение и путь, и время равны нулю: 0/0.
Аудитория     : школьная алгебра; анализ не предполагается.
Ключевая идея : производная — это не «деление на ноль», а ПРЕДЕЛ наклонов
                секущих; предел описывает поведение ВБЛИЗИ точки, а не в ней.
Цвета         : функция — синий; h / Δx / δ — зелёный; Δy — красный;
                секущая/касательная — жёлтый; ε — розовый; предел L — бирюзовый.

 0. Вступление     — машинка, спидометр, «0/0?», карта пути.
 1. Средняя скорость — s(t)=t², Δs/Δt на [1;3] и [1;2].
 2. Наклон секущей — подъём/пробег, формула (f(x+h)−f(x))/h.
 3. Ловушка 0/0    — h→0: таблица 3; 2,5; 2,1; 2,01…; h=0 даёт 0/0;
                     «0·?=0»; алгебра: 2+h; график k(h) с «дыркой».
 4. Предел: интуиция — точки подходят слева и справа; значение в самой точке
                     не важно; sin x / x → 1; предела нет: скачок, sin(1/x).
 5. Предел: ε–δ    — формула по частям, игра «скептик выбирает ε — мы δ»,
                     полосы на графике, доказательство для 2+h, контрпример.
 6. Производная    — определение, обозначения Лагранжа/Лейбница, секущие →
                     касательная, вывод (x²)'=2x, график f' «прорисовывается».
 7. Когда её нет   — «лупа»: гладкая кривая выпрямляется, |x| — нет;
                     угол, вертикальная касательная, разрыв, функция Вейерштрасса.
 8. Константа и сумма  — из определения; наглядно «столбики».
 9. Степень        — квадрат x²→(x+h)², бином, (xⁿ)'=nxⁿ⁻¹.
10. Произведение   — прямоугольник f·g, приём «прибавить-вычесть».
11. Цепное правило — три числовые прямые, растяжения перемножаются.
12. Экспонента     — (aˣ)' = aˣ·M(a); поиск a, где M(a)=1 → e.
13. Логарифм       — зеркало y=x, два вывода, (aˣ)'=aˣ ln a, частное, сводка.
14. Применения     — падение (скорость/ускорение), забор (максимум площади),
                     линейное приближение √4,1.
15. Итог           — цепочка идей и финальная формула.
"""

from manim import *
import numpy as np

# ───────────────────────────── стиль ──────────────────────────────────────
C_BG = "#0F1115"
C_F = BLUE_C        # функция
C_F2 = TEAL_C       # вторая функция
C_H = GREEN_C       # h, Δx, δ
C_DY = RED_C        # Δy
C_SEC = YELLOW      # секущая / касательная
C_EPS = "#FF79B0"   # ε
C_LIM = TEAL_B      # предел L
C_BAD = RED         # ошибки, «нельзя»
C_GOOD = GREEN_B

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
    """Русский текст (Pango)."""
    return Text(text, font=FONT, font_size=size, color=color, **kw)


def M(*tex, size=44, color=WHITE, **kw):
    """Формула (LaTeX, кириллица разрешена внутри \\text{})."""
    return MathTex(*tex, font_size=size, color=color, **kw)


def ru(v, d=3):
    """Число в русской записи: запятая, типографский минус."""
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


def unit(v):
    n = np.linalg.norm(v)
    return v / n if n > 1e-9 else RIGHT


def live(getter, place, fmt="{}", d=3, size=34, color=WHITE):
    """Живое число на Pango (быстро, без LaTeX на каждом кадре).
    fmt содержит {} на месте числа, place(mob) расставляет объект."""

    def build():
        m = T(fmt.format(ru(getter(), d)), size, color)
        place(m)
        return m

    return always_redraw(build)


def secant(ax, f, x, h, length=6.5, color=C_SEC, legs=True):
    p, q = ax.c2p(x, f(x)), ax.c2p(x + h, f(x + h))
    d = unit(q - p)
    mid = (p + q) / 2 if np.linalg.norm(q - p) > 1e-3 else p
    g = VGroup(Line(mid - d * length / 2, mid + d * length / 2, color=color, stroke_width=3))
    if legs:
        c = ax.c2p(x + h, f(x))
        g.add(Line(p, c, color=C_H, stroke_width=4), Line(c, q, color=C_DY, stroke_width=4))
    g.add(Dot(p, radius=0.06), Dot(q, radius=0.06))
    return g


def tangent(ax, f, fp, x, length=4.0, color=C_SEC, width=3):
    p = ax.c2p(x, f(x))
    d = unit(ax.c2p(x + 1, f(x) + fp(x)) - p)
    return Line(p - d * length / 2, p + d * length / 2, color=color, stroke_width=width)


def band(ax, x0, x1, y0, y1, color, opacity=0.2):
    p0, p1 = ax.c2p(x0, y0), ax.c2p(x1, y1)
    r = Rectangle(width=max(abs(p1[0] - p0[0]), 1e-3), height=max(abs(p1[1] - p0[1]), 1e-3))
    r.set_stroke(width=0).set_fill(color, opacity)
    return r.move_to((p0 + p1) / 2)


def hole(point, color=C_F, r=0.08):
    return Circle(radius=r, color=color, stroke_width=3).set_fill(C_BG, 1).move_to(point)


def std_axes(xr, yr, xl, yl, nums=True, font=22):
    return Axes(
        x_range=xr, y_range=yr, x_length=xl, y_length=yl, tips=False,
        axis_config={"include_numbers": nums, "font_size": font, "color": GREY_B,
                     "stroke_width": 2},
    )


# ═════════════════════════ базовая сцена ══════════════════════════════════
class Story(Scene):
    """Общие приёмы: субтитры, заставки глав, очистка, пошаговые выкладки."""

    def setup(self):
        self.cap = None

    # --- субтитры -------------------------------------------------------
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
        line = Line(LEFT * 4, RIGHT * 4, color=C_SEC, stroke_width=2).next_to(g, DOWN, buff=0.35)
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

    # --- выкладка «строка за строкой» ------------------------------------
    def derive(self, lhs, steps, size=36, buff=0.3, pos=ORIGIN, max_h=5.2, max_w=12.8):
        """lhs = rhs0 \n = rhs1 \n ... ; steps: [(tex, подпись|None), ...]"""
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

    @staticmethod
    def make_car(color=C_SEC):
        body = RoundedRectangle(corner_radius=0.08, width=1.0, height=0.28)
        body.set_fill(color, 1).set_stroke(width=0)
        cabin = RoundedRectangle(corner_radius=0.08, width=0.5, height=0.22)
        cabin.set_fill(color, 1).set_stroke(width=0)
        cabin.next_to(body, UP, buff=-0.02).shift(0.08 * LEFT)
        w1 = Circle(radius=0.11).set_fill(GREY_D, 1).set_stroke(WHITE, 2)
        w1.move_to(body.get_bottom() + 0.28 * LEFT)
        w2 = w1.copy().move_to(body.get_bottom() + 0.28 * RIGHT)
        return VGroup(body, cabin, w1, w2)

    # ═══════════════════════ 0. Вступление ═══════════════════════════════
    def ch00_intro(self):
        title = T("Производная", 88)
        sub = T("и как к ней приходят через пределы", 36, GREY_A)
        VGroup(title, sub).arrange(DOWN, buff=0.45)
        self.play(Write(title), run_time=2)
        self.play(FadeIn(sub, shift=0.2 * UP))
        self.wait(2)
        self.play(FadeOut(title), FadeOut(sub))

        road = NumberLine(x_range=[0, 9, 1], length=11.5, include_numbers=True,
                          font_size=24, color=GREY_B).shift(UP * 1.2)
        road_lbl = T("путь s, м", 24, GREY_B).next_to(road, DOWN, buff=0.45).align_to(road, RIGHT)
        car = self.make_car()
        t = ValueTracker(0)
        car.add_updater(lambda m: m.move_to(road.n2p(t.get_value() ** 2) + 0.36 * UP))
        self.play(Create(road), FadeIn(road_lbl), FadeIn(car, shift=DOWN * 0.3))

        law = M(r"s(t)=t^2", size=40, color=C_F).to_corner(UL, buff=0.6)
        clock = live(lambda: t.get_value(), lambda m: m.move_to(LEFT * 3 + DOWN * 0.6),
                     "время  t = {} с", d=2, size=34)
        speedo = live(lambda: 2 * t.get_value(), lambda m: m.move_to(RIGHT * 3 + DOWN * 0.6),
                      "спидометр: {} м/с", d=1, size=34, color=C_SEC)
        self.play(Write(law), FadeIn(clock), FadeIn(speedo))
        self.say("Машина едет по прямой. Её положение в момент t задаётся "
                 "формулой s(t) = t²: за 1 с — 1 м, за 2 с — 4 м, за 3 с — 9 м.")
        self.play(t.animate.set_value(1), run_time=2.5, rate_func=linear)
        self.say("Стоп! Ровно в момент t = 1 спидометр показывает 2 м/с. "
                 "Что на самом деле означает это число?")
        self.play(t.animate.set_value(3), run_time=3.5, rate_func=linear)
        self.wait(0.5)

        for m in (clock, speedo, car):
            m.clear_updaters()
        self.play(*[FadeOut(m) for m in (road, road_lbl, car, clock, speedo, law)])

        v = M(r"\text{скорость}", "=", r"\frac{\text{путь}}{\text{время}}", size=56)
        self.play(Write(v))
        self.say("Со школы мы знаем: скорость — это путь, делённый на время. "
                 "Но за какой промежуток времени?")
        inst = M(r"\text{скорость в момент}", "=", r"\frac{0\ \text{м}}{0\ \text{с}}", "=", "?", size=56)
        inst[2].set_color(C_BAD)
        inst[4].set_color(C_BAD)
        self.play(TransformMatchingTex(v, inst))
        self.say("«В мгновение» машина проезжает 0 метров за 0 секунд. "
                 "Получается 0/0 — выражение, которое не имеет смысла.")
        self.say("Весь математический анализ начинается с того, как аккуратно "
                 "обойти эту ловушку. Инструмент для этого — ПРЕДЕЛ.")
        self.play(FadeOut(inst))
        self.unsay()

        plan_items = [
            "средняя скорость", "наклон секущей", "ловушка 0/0",
            "предел: интуиция", "предел: строго (ε–δ)", "определение производной",
            "когда производной нет", "правила дифференцирования", "применения",
        ]
        head = T("Наш маршрут", 44, C_SEC).to_edge(UP, buff=0.5)
        boxes = VGroup()
        for i, s in enumerate(plan_items):
            txt = T(f"{i + 1}. {s}", 26)
            box = SurroundingRectangle(txt, buff=0.16, corner_radius=0.1,
                                       color=GREY_B, stroke_width=1.5)
            boxes.add(VGroup(box, txt))
        boxes.arrange_in_grid(rows=3, cols=3, buff=(0.45, 0.6))
        boxes.scale_to_fit_width(12.8).next_to(head, DOWN, buff=0.7)
        arrows = VGroup()
        order = list(boxes)
        for a, b in zip(order[:-1], order[1:]):
            if abs(a.get_y() - b.get_y()) < 0.1:
                arrows.add(Arrow(a.get_right(), b.get_left(), buff=0.06,
                                 stroke_width=3, color=GREY_B, max_tip_length_to_length_ratio=0.3))
        self.play(Write(head))
        self.play(LaggedStart(*[FadeIn(b, shift=0.2 * UP) for b in boxes], lag_ratio=0.25), run_time=3)
        self.play(LaggedStart(*[Create(a) for a in arrows], lag_ratio=0.2), run_time=2)
        self.say("Мы пройдём весь путь: от школьной средней скорости до строгого "
                 "определения предела, выведем все основные правила из определения "
                 "и применим их к настоящим задачам.")
        self.wait(1)

    # ═══════════════════════ 1. Средняя скорость ═════════════════════════
    def ch01_average_speed(self):
        self.chapter(1, "Средняя скорость", "с чего всё начинается")
        f = lambda t: t ** 2
        ax = std_axes([0, 3.2, 1], [0, 10, 2], 5.6, 5.2).shift(LEFT * 3.4 + UP * 0.55)
        xl = ax.get_x_axis_label(M(r"t,\ \text{с}", size=30), edge=RIGHT, direction=DR)
        yl = ax.get_y_axis_label(M(r"s,\ \text{м}", size=30), edge=UP, direction=UL)
        graph = ax.plot(f, x_range=[0, 3.1], color=C_F, stroke_width=4)
        glabel = M(r"s(t)=t^2", size=36, color=C_F).next_to(ax.c2p(2.3, 8.5), LEFT)
        self.play(Create(ax), Write(xl), Write(yl))
        self.play(Create(graph), Write(glabel), run_time=2)
        self.say("Нарисуем график положения. По горизонтали — время, "
                 "по вертикали — пройденный путь.")

        # «дорога» справа от графика — вертикальная прямая
        road = Line(ax.c2p(3.55, 0), ax.c2p(3.55, 10), color=GREY_C, stroke_width=6)
        t = ValueTracker(0)
        pdot = always_redraw(lambda: Dot(ax.c2p(t.get_value(), f(t.get_value())), color=C_SEC))
        cdot = always_redraw(lambda: Dot(ax.c2p(3.55, f(t.get_value())), color=C_SEC, radius=0.11))
        link = always_redraw(lambda: DashedLine(ax.c2p(t.get_value(), f(t.get_value())),
                                                ax.c2p(3.55, f(t.get_value())),
                                                color=GREY_B, stroke_width=2))
        self.play(Create(road), FadeIn(pdot), FadeIn(cdot), FadeIn(link))
        self.play(t.animate.set_value(3), run_time=4, rate_func=linear)
        self.say("Точка на графике и машина на «дороге» всегда на одной высоте. "
                 "Чем круче график — тем быстрее едет машина.")
        for m in (pdot, cdot, link):
            m.clear_updaters()
        self.play(FadeOut(pdot), FadeOut(cdot), FadeOut(link), FadeOut(road))

        # средняя скорость на [1; 3]
        form = M(r"v_{\text{ср}}", "=", r"\frac{\Delta s}{\Delta t}", size=46).move_to(RIGHT * 3.6 + UP * 2.6)
        form[2][0:2].set_color(C_DY)
        form[2][3:5].set_color(C_H)
        self.play(Write(form))
        self.say("Средняя скорость на промежутке — это изменение пути Δs, "
                 "делённое на изменение времени Δt.")

        def legs(t1, t2):
            p, c, q = ax.c2p(t1, f(t1)), ax.c2p(t2, f(t1)), ax.c2p(t2, f(t2))
            return VGroup(Line(p, c, color=C_H, stroke_width=4), Line(c, q, color=C_DY, stroke_width=4),
                          Dot(p), Dot(q))

        t2 = ValueTracker(3)
        lg = always_redraw(lambda: legs(1, t2.get_value()))
        dt_lbl = always_redraw(lambda: M(r"\Delta t", size=30, color=C_H).next_to(
            ax.c2p((1 + t2.get_value()) / 2, 1), DOWN, buff=0.12))
        ds_lbl = always_redraw(lambda: M(r"\Delta s", size=30, color=C_DY).next_to(
            ax.c2p(t2.get_value(), (1 + f(t2.get_value())) / 2), RIGHT, buff=0.12))
        self.play(FadeIn(lg), FadeIn(dt_lbl), FadeIn(ds_lbl))

        calc1 = M(r"[1;\,3]:\quad v_{\text{ср}}=\frac{9-1}{3-1}=4\ \text{м/с}", size=36)
        calc2 = M(r"[1;\,2]:\quad v_{\text{ср}}=\frac{4-1}{2-1}=3\ \text{м/с}", size=36)
        calc3 = M(r"[1;\,1{,}5]:\quad v_{\text{ср}}=\frac{2{,}25-1}{0{,}5}=2{,}5\ \text{м/с}", size=36)
        calcs = VGroup(calc1, calc2, calc3).arrange(DOWN, aligned_edge=LEFT, buff=0.45)
        calcs.next_to(form, DOWN, buff=0.6)
        if calcs.width > 6.6:
            calcs.scale_to_fit_width(6.6)
        calcs.set_x(3.6)
        self.play(Write(calc1))
        self.say("С первой по третью секунду машина проехала 8 м за 2 с — в среднем 4 м/с.")
        self.play(t2.animate.set_value(2), run_time=2)
        self.play(Write(calc2))
        self.say("С первой по вторую — 3 м за 1 с: в среднем 3 м/с. Уже меньше.")
        self.play(t2.animate.set_value(1.5), run_time=2)
        self.play(Write(calc3))
        self.say("Чем короче промежуток, тем меньше машина успевает разогнаться, "
                 "и тем ближе средняя скорость к скорости «в момент t = 1».")
        q = T("А какая скорость РОВНО при t = 1?", 30, C_SEC)
        q.scale_to_fit_width(min(q.width, 6.2)).next_to(calcs, DOWN, buff=0.45).set_x(3.8)
        self.play(Write(q))
        self.say("Средняя скорость — понятие простое. Мгновенная — вот загадка. "
                 "Чтобы её решить, посмотрим на картинку геометрически.")

    # ═══════════════════════ 2. Наклон секущей ═══════════════════════════
    def ch02_secant(self):
        self.chapter(2, "Наклон секущей", "средняя скорость — это геометрия")
        F = lambda x: 0.2 * x ** 3 - 1.2 * x ** 2 + 2 * x + 1
        ax = std_axes([0, 5, 1], [0, 6, 1], 6.2, 5.2).shift(LEFT * 3.1 + UP * 0.55)
        graph = ax.plot(F, x_range=[0, 4.95], color=C_F, stroke_width=4)
        gl = M("y=f(x)", size=34, color=C_F).next_to(ax.c2p(4.9, 5.7), LEFT)
        self.play(Create(ax), Create(graph), Write(gl), run_time=2)
        self.say("Возьмём теперь любую функцию y = f(x) и две точки на её графике.")

        x0, h = 1.0, ValueTracker(2.5)
        sec = always_redraw(lambda: secant(ax, F, x0, h.get_value()))
        hl = always_redraw(lambda: M("h", size=32, color=C_H).next_to(
            ax.c2p(x0 + h.get_value() / 2, F(x0)), DOWN, buff=0.12))
        dyl = always_redraw(lambda: M(r"\Delta y", size=32, color=C_DY).next_to(
            ax.c2p(x0 + h.get_value(), (F(x0) + F(x0 + h.get_value())) / 2), RIGHT, buff=0.12))
        self.play(FadeIn(sec), FadeIn(hl), FadeIn(dyl))

        P = M("(x,\ f(x))", size=28).next_to(ax.c2p(x0, F(x0)), UL, buff=0.1)
        Q = M("(x+h,\ f(x+h))", size=28).next_to(ax.c2p(x0 + 2.5, F(x0 + 2.5)), LEFT, buff=0.2).shift(UP * 0.3)
        self.play(Write(P), Write(Q))
        self.say("Первая точка — (x, f(x)). Сдвинемся вправо на h: вторая "
                 "точка — (x + h, f(x + h)). Прямая через них называется секущей.")

        s1 = M(r"\text{наклон}", "=", r"\frac{\text{подъём}}{\text{пробег}}", size=38)
        s2 = M(r"\text{наклон}", "=", r"\frac{\Delta y}{\Delta x}", size=38)
        s3 = M(r"\text{наклон}", "=", r"\frac{f(x+h)-f(x)}{h}", size=38)
        for s in (s1, s2, s3):
            s.move_to(RIGHT * 3.7 + UP * 2.2)
        self.play(Write(s1))
        self.say("Наклон прямой — это «подъём, делённый на пробег»: насколько "
                 "прямая поднимается при сдвиге на единицу вправо.")
        self.play(TransformMatchingTex(s1, s2))
        self.play(TransformMatchingShapes(s2, s3))
        s3[2][0:9].set_color(C_DY)
        s3[2][-1].set_color(C_H)
        box = SurroundingRectangle(s3, color=C_SEC, buff=0.2)
        self.play(Create(box))
        self.say("Подъём — это f(x + h) − f(x), пробег — h. Эту дробь "
                 "называют разностным отношением. Запомните её — это главный герой.")

        phys = VGroup(
            T("Для s(t) = t²:", 28, GREY_A),
            T("наклон секущей = средняя скорость", 28, C_SEC),
        ).arrange(DOWN, buff=0.2).next_to(box, DOWN, buff=0.7)
        self.play(FadeIn(phys, shift=UP * 0.2))
        self.say("Если функция — это путь от времени, то наклон секущей — ровно "
                 "средняя скорость Δs/Δt. Та же самая дробь!")
        self.play(FadeOut(P), FadeOut(Q))
        self.play(h.animate.set_value(1.2), run_time=2)
        self.play(h.animate.set_value(3.3), run_time=2)
        self.play(h.animate.set_value(0.6), run_time=2)
        self.say("Меняя h, мы вращаем секущую вокруг точки (x, f(x)). "
                 "А что будет, если h станет совсем маленьким?")

    # ═══════════════════════ 3. Ловушка 0/0 ══════════════════════════════
    def ch03_zero_over_zero(self):
        self.chapter(3, "Ловушка 0/0", "почему нельзя просто взять h = 0")
        f = lambda x: x ** 2
        ax = std_axes([0, 2.5, 1], [0, 6, 1], 5.2, 5.2).shift(LEFT * 4.0 + UP * 0.55)
        graph = ax.plot(f, x_range=[0, 2.4], color=C_F, stroke_width=4)
        gl = M("f(x)=x^2", size=34, color=C_F).next_to(ax.c2p(2.4, 5.8), LEFT)
        lh = ValueTracker(0)  # log10(h)
        H = lambda: 10 ** lh.get_value()
        sec = always_redraw(lambda: secant(ax, f, 1, H(), length=6))
        self.play(Create(ax), Create(graph), Write(gl))
        self.play(FadeIn(sec))
        self.say("Вернёмся к f(x) = x² и точке x = 1. Будем уменьшать h "
                 "и смотреть, что происходит с наклоном секущей.")

        vals = [(0, "1", "3"), (np.log10(0.5), "0{,}5", "2{,}5"), (-1, "0{,}1", "2{,}1"),
                (-2, "0{,}01", "2{,}01"), (-3, "0{,}001", "2{,}001")]
        head = [M("h", size=36, color=C_H), M(r"\frac{f(1+h)-f(1)}{h}", size=32, color=C_SEC)]
        cells = []
        for _, a, b in vals:
            cells += [M(a, size=34), M(b, size=34)]
        grid = VGroup(*head, *cells).arrange_in_grid(rows=6, cols=2, buff=(1.1, 0.3))
        grid.move_to(RIGHT * 2.2 + UP * 0.6)
        hline = Line(grid.get_left() + LEFT * 0.2, grid.get_right() + RIGHT * 0.2,
                     color=GREY_B, stroke_width=2).next_to(VGroup(*head), DOWN, buff=0.15)
        hline.set_x(grid.get_x())
        self.play(FadeIn(VGroup(*head)), Create(hline))
        for i, (lv, _, _) in enumerate(vals):
            self.play(lh.animate.set_value(lv), run_time=1.5)
            self.play(FadeIn(cells[2 * i]), FadeIn(cells[2 * i + 1], shift=LEFT * 0.2), run_time=0.7)
        self.say("Наклоны: 3; 2,5; 2,1; 2,01; 2,001… Они явно стремятся к числу 2. "
                 "А секущая почти слилась с касательной.")
        arrow = MathTex(r"\to 2", font_size=44, color=C_LIM).next_to(grid, RIGHT, buff=0.3).shift(DOWN * 1.2)
        self.play(Write(arrow))

        self.say("Так почему бы просто не подставить h = 0?")
        sec.clear_updaters()
        zero = M(r"\frac{f(1+0)-f(1)}{0}", "=", r"\frac{1-1}{0}", "=", r"\frac{0}{0}", size=48)
        zero[4].set_color(C_BAD)
        zero.move_to(RIGHT * 2.8 + DOWN * 0.3)
        self.play(FadeOut(grid), FadeOut(hline), FadeOut(arrow))
        self.play(Write(zero))
        self.say("При h = 0 обе точки совпадают. Через одну точку проходит "
                 "бесконечно много прямых, и формула выдаёт 0/0.")

        self.play(FadeOut(zero))
        eq = M(r"0\cdot", "5", "=0", size=60).move_to(RIGHT * 2.6 + UP * 0.8)
        eq[1].set_color(C_SEC)
        q = M(r"\frac{0}{0}=\ ?", size=60, color=C_BAD).next_to(eq, DOWN, buff=0.8)
        self.play(Write(q))
        self.say("Делить — значит искать число, которое при умножении на делитель "
                 "даёт делимое. Какое число «?» подходит под 0 · ? = 0?")
        self.play(Write(eq))
        for s in ["2", "-3", r"\pi", "1000"]:
            new = M(r"0\cdot", s, "=0", size=60).move_to(eq)
            new[1].set_color(C_SEC)
            self.play(TransformMatchingTex(eq, new), run_time=0.7)
            eq = new
            self.wait(0.4)
        self.say("Подходит ЛЮБОЕ число! Поэтому 0/0 не равно ничему конкретному. "
                 "Это не ответ, а сигнал: «нужен другой способ».")
        self.play(FadeOut(eq), FadeOut(q), FadeOut(sec))

        rows = self.derive(
            r"\frac{f(1+h)-f(1)}{h}",
            [(r"\frac{(1+h)^2-1}{h}", None),
             (r"\frac{1+2h+h^2-1}{h}", "Раскроем скобки."),
             (r"\frac{2h+h^2}{h}", "Единицы сократились."),
             (r"\frac{h\,(2+h)}{h}", "Вынесем h за скобку…"),
             (r"2+h \qquad (h\neq 0)", "…и сократим. Это законно, пока h ≠ 0!")],
            size=34, pos=RIGHT * 2.6 + UP * 0.4, max_h=5.4, max_w=7.2)
        self.say("Для любого h ≠ 0 наклон секущей равен ровно 2 + h. "
                 "При h, близком к нулю, это число близко к 2.")
        self.clear_all()

        kax = std_axes([-1.5, 1.5, 0.5], [0, 4, 1], 7, 4.6, nums=False).shift(UP * 0.5)
        kax.add_coordinates({-1: M("-1", size=24), 1: M("1", size=24)},
                            {1: M("1", size=24), 2: M("2", size=24), 3: M("3", size=24)})
        kl = kax.get_x_axis_label(M("h", size=32, color=C_H))
        kyl = kax.get_y_axis_label(M(r"k(h)", size=32, color=C_SEC), edge=UP, direction=RIGHT, buff=0.3)
        kg = kax.plot(lambda x: 2 + x, x_range=[-1.5, 1.5], color=C_SEC, stroke_width=4)
        hh = hole(kax.c2p(0, 2), C_SEC)
        kf = M(r"k(h)=\frac{f(1+h)-f(1)}{h}=2+h,\quad h\ne 0", size=34).to_edge(UP, buff=0.35)
        self.play(Create(kax), Write(kl), Write(kyl), Write(kf))
        self.play(Create(kg), run_time=1.5)
        self.play(FadeIn(hh, scale=2))
        self.say("Нарисуем наклон секущей как функцию k(h). Это прямая 2 + h, "
                 "но с «дыркой» при h = 0: там функция просто не определена.")
        self.play(Indicate(hh, color=C_BAD, scale_factor=2))
        self.say("Мы не можем спросить «чему равно k(0)?». Но мы можем спросить: "
                 "«к чему стремится k(h), когда h подходит к нулю?»")
        self.say("Ответ — 2. Этот новый вопрос и есть понятие ПРЕДЕЛА.")

    # ═══════════════════════ 4. Предел: интуиция ═════════════════════════
    def ch04_limit_intuition(self):
        self.chapter(4, "Предел: интуиция", "что происходит ВБЛИЗИ точки")
        ax = std_axes([-1.5, 1.5, 0.5], [0, 4, 1], 7, 4.4, nums=False).shift(UP * 0.35 + LEFT * 1.6)
        ax.add_coordinates({-1: M("-1", size=24), 1: M("1", size=24)},
                           {1: M("1", size=24), 2: M("2", size=24), 3: M("3", size=24)})
        g = lambda x: 2 + x
        kg = ax.plot(g, x_range=[-1.5, 1.5], color=C_SEC, stroke_width=4)
        hh = hole(ax.c2p(0, 2), C_SEC)
        self.play(Create(ax), Create(kg), FadeIn(hh))

        d = ValueTracker(1.3)

        def approach(sign, col):
            def build():
                x = sign * d.get_value()
                p = ax.c2p(x, g(x))
                return VGroup(Dot(p, color=col, radius=0.08),
                              DashedLine(p, ax.c2p(0, g(x)), color=col, stroke_width=2),
                              DashedLine(p, ax.c2p(x, 0), color=col, stroke_width=2))
            return always_redraw(build)

        L = approach(-1, C_H)
        R = approach(1, C_EPS)
        pl = lambda m: m.to_edge(RIGHT, buff=0.6).shift(UP * 1.5)
        lv = live(lambda: g(-d.get_value()), lambda m: m.to_edge(RIGHT, buff=0.7).shift(UP * 1.4),
                  "слева: {}", size=32, color=C_H)
        rv = live(lambda: g(d.get_value()), lambda m: m.to_edge(RIGHT, buff=0.7).shift(UP * 0.6),
                  "справа: {}", size=32, color=C_EPS)
        self.play(FadeIn(L), FadeIn(R), FadeIn(lv), FadeIn(rv))
        self.say("Пустим две точки к нулю — слева и справа — и будем следить "
                 "за значениями функции.")
        self.play(d.animate.set_value(0.02), run_time=5, rate_func=rate_functions.ease_in_out_sine)
        self.say("Оба значения сходятся к 2 — хотя в самой точке h = 0 функции нет.")

        defn = M(r"\lim_{h\to 0} k(h) = 2", size=46, color=C_LIM).to_edge(RIGHT, buff=0.6).shift(DOWN * 0.8)
        self.play(Write(defn))
        self.say("Записывают так: предел k(h) при h, стремящемся к нулю, равен 2. "
                 "«lim» — от латинского limes, «граница».")
        self.play(FadeOut(L), FadeOut(R), FadeOut(lv), FadeOut(rv))
        L.clear_updaters(), R.clear_updaters()

        gen = VGroup(
            M(r"\lim_{x\to a} f(x)=L", size=46),
            T("значения f(x) сколь угодно близки к L,", 28),
            T("если x достаточно близко к a (но x ≠ a)", 28),
        ).arrange(DOWN, buff=0.25)
        gen[0].set_color(C_LIM)
        gen.to_edge(RIGHT, buff=0.35).shift(DOWN * 0.3)
        self.play(ReplacementTransform(defn, gen[0]))
        self.play(FadeIn(gen[1:], shift=UP * 0.2))
        self.say("Общее определение «на словах»: f(x) можно сделать сколь угодно "
                 "близким к L, взяв x достаточно близко к a.")
        rect = SurroundingRectangle(gen[2][-7:], color=C_BAD, buff=0.06)
        self.play(Create(rect))
        self.say("Важнейшая оговорка: x ≠ a. Предел ничего не знает о значении "
                 "в самой точке — только о поведении рядом с ней.")

        weird = Dot(ax.c2p(0, 3.4), color=C_BAD, radius=0.09)
        wl = M("k(0)=3{,}4", size=28, color=C_BAD).next_to(weird, RIGHT, buff=0.15)
        self.play(FadeIn(weird, scale=2), Write(wl))
        self.say("Допустим, кто-то доопределил k(0) = 3,4. Предел от этого "
                 "не изменится: он по-прежнему равен 2.")
        self.play(FadeOut(weird), FadeOut(wl), FadeOut(rect))
        self.clear_all()

        # --- sin x / x
        sax = std_axes([-8, 8, 2], [-0.4, 1.2, 0.5], 11, 3.8, nums=False).shift(UP * 1.0)
        sax.add_coordinates({-2 * i: M(str(-2 * i), size=22) for i in range(1, 4)} |
                            {2 * i: M(str(2 * i), size=22) for i in range(1, 4)}, {1: M("1", size=22)})
        sg = sax.plot(lambda x: np.sin(x) / x if abs(x) > 1e-6 else 1, x_range=[-8, 8, 0.02],
                      color=C_F, stroke_width=4)
        sh = hole(sax.c2p(0, 1), C_F)
        st = M(r"f(x)=\frac{\sin x}{x}", size=40, color=C_F).to_corner(UL, buff=0.45)
        self.play(Create(sax), Write(st))
        self.play(Create(sg), run_time=2)
        self.play(FadeIn(sh, scale=2))
        self.say("Знаменитый пример: sin x / x. При x = 0 снова 0/0, "
                 "график с «дыркой». Но куда он стремится?")
        tbl = VGroup(
            M(r"x=0{,}5:\ \ 0{,}9589", size=32),
            M(r"x=0{,}1:\ \ 0{,}99833", size=32),
            M(r"x=0{,}01:\ \ 0{,}9999833", size=32),
        ).arrange(RIGHT, buff=0.8).next_to(sax, DOWN, buff=0.35)
        self.play(LaggedStart(*[FadeIn(t, shift=UP * 0.2) for t in tbl], lag_ratio=0.5))
        res = M(r"\lim_{x\to 0}\frac{\sin x}{x}=1", size=44, color=C_LIM).next_to(tbl, DOWN, buff=0.3)
        self.play(Write(res))
        self.say("Значения подбираются к 1. Предел равен 1, хотя в нуле "
                 "функция не определена.")
        self.clear_all()

        # --- когда предела нет
        head = T("Когда предела нет", 40, C_BAD).to_edge(UP, buff=0.4)
        self.play(Write(head))
        jx = std_axes([-2, 2, 1], [-1.6, 1.6, 1], 5.2, 3.8, nums=False).shift(LEFT * 3.3 + UP * 0.1)
        j1 = jx.plot(lambda x: -1, x_range=[-2, 0], color=C_F, stroke_width=4)
        j2 = jx.plot(lambda x: 1, x_range=[0, 2], color=C_F, stroke_width=4)
        jh = VGroup(hole(jx.c2p(0, -1)), hole(jx.c2p(0, 1)))
        jt = T("скачок", 30).next_to(jx, DOWN, buff=0.25)
        ox = std_axes([-1, 1, 1], [-1.3, 1.3, 1], 5.2, 3.8, nums=False).shift(RIGHT * 3.3 + UP * 0.1)
        o1 = ox.plot(lambda x: np.sin(1 / x), x_range=[0.012, 1, 0.0002], color=C_F,
                     stroke_width=2, use_smoothing=False)
        o2 = ox.plot(lambda x: np.sin(1 / x), x_range=[-1, -0.012, 0.0002], color=C_F,
                     stroke_width=2, use_smoothing=False)
        ot = M(r"\sin\frac{1}{x}", size=36).next_to(ox, DOWN, buff=0.25)
        self.play(Create(jx), Create(j1), Create(j2), FadeIn(jh), Write(jt))
        self.say("Слева значения равны −1, справа — +1. Односторонние пределы "
                 "разные, единого числа, к которому всё стремится, нет.")
        self.play(Create(ox), Create(o1), Create(o2), Write(ot), run_time=3)
        self.say("А sin(1/x) у нуля колеблется всё быстрее, пробегая все значения "
                 "от −1 до 1. Он ни к чему не стремится.")
        self.say("Но что значат слова «сколь угодно близко» и «достаточно близко»? "
                 "Чтобы на них опираться, нужна точность.")

    # ═══════════════════════ 5. Предел: ε–δ ══════════════════════════════
    def ch05_epsilon_delta(self):
        self.chapter(5, "Предел: строгое определение", "язык ε–δ (Коши, Вейерштрасс)")
        l1 = M(r"\lim_{x\to a} f(x)=L", r"\quad\iff", size=40)
        l2 = M(r"\forall\varepsilon>0", r"\ \ \exists\delta>0", r":\quad",
               r"0<|x-a|<\delta", r"\ \Rightarrow\ ", r"|f(x)-L|<\varepsilon", size=40)
        for i, c in [(0, C_EPS), (1, C_H), (3, C_H), (5, C_EPS)]:
            l2[i].set_color(c)
        formula = VGroup(l1, l2).arrange(DOWN, buff=0.3).move_to(UP * 1.2)
        self.play(Write(l1))
        self.play(Write(l2), run_time=3)
        self.say("Вот строгое определение предела. Выглядит пугающе, "
                 "но разберём его по кусочкам.")
        legend = VGroup(*[VGroup(M(a, size=36, color=C_SEC), T(b, 26)).arrange(RIGHT, buff=0.3) for a, b in [
            (r"\forall", "«для любого»"), (r"\exists", "«найдётся»"),
            (r"\Rightarrow", "«значит»"), (r"|x-a|", "расстояние между x и a"),
            (r"\varepsilon,\ \delta", "«эпсилон», «дельта» — просто маленькие числа"),
        ]]).arrange_in_grid(rows=3, cols=2, buff=(0.8, 0.3), flow_order="dr").next_to(formula, DOWN, buff=0.6)
        self.play(LaggedStart(*[FadeIn(x, shift=UP * 0.2) for x in legend], lag_ratio=0.3))
        self.say("Сначала словарик: перевернутая A — «для любого», "
                 "перевёрнутая E — «найдётся», стрелка — «значит».")
        self.play(FadeOut(legend))
        parts = [
            (0, "«Для любого ε > 0» — какую бы точность ε нам ни задали…"),
            (1, "«…найдётся δ > 0» — мы можем подобрать такое расстояние δ…"),
            (3, "«…что если x отличается от a меньше чем на δ (и x ≠ a)…»"),
            (5, "«…то f(x) отличается от L меньше чем на ε»."),
        ]
        prev = None
        for i, text in parts:
            r = SurroundingRectangle(l2[i], color=WHITE, buff=0.08)
            self.play(Create(r) if prev is None else ReplacementTransform(prev, r))
            self.say(text)
            prev = r
        self.play(FadeOut(prev))
        game = VGroup(T("Это игра двух игроков:", 30),
                      T("скептик называет ε, а мы должны ответить δ.", 30, C_SEC),
                      T("Предел равен L, если мы выигрываем при ЛЮБОМ ε.", 30)).arrange(DOWN, buff=0.2)
        game.next_to(formula, DOWN, buff=0.6)
        self.play(FadeIn(game, shift=UP * 0.2))
        self.wait(4)
        self.play(FadeOut(game), formula.animate.scale(0.75).to_edge(UP, buff=0.25))
        self.unsay()

        f = lambda x: 0.25 * x ** 2 + 1
        finv = lambda y: 2 * np.sqrt(max(y - 1, 0))
        a, Lv = 2.0, 2.0
        ax = std_axes([0, 4, 1], [0, 5, 1], 6.2, 3.9).shift(LEFT * 3.0 + DOWN * 0.2)
        graph = ax.plot(f, x_range=[0, 4], color=C_F, stroke_width=4)
        self.play(Create(ax), Create(graph))
        am = DashedLine(ax.c2p(a, 0), ax.c2p(a, Lv), color=GREY_B)
        Lm = DashedLine(ax.c2p(0, Lv), ax.c2p(a, Lv), color=GREY_B)
        al = M("a", size=30).next_to(ax.c2p(a, 0), DOWN, buff=0.4)
        Ll = M("L", size=30, color=C_LIM).next_to(ax.c2p(0, Lv), LEFT, buff=0.4)
        self.play(Create(am), Create(Lm), Write(al), Write(Ll))

        eps = ValueTracker(1.0)
        delta = lambda: min(a - finv(Lv - eps.get_value()), finv(Lv + eps.get_value()) - a)
        eb = always_redraw(lambda: band(ax, 0, 4, Lv - eps.get_value(), Lv + eps.get_value(), C_EPS, 0.2))
        db = always_redraw(lambda: band(ax, a - delta(), a + delta(), 0, 5, C_H, 0.2))
        piece = always_redraw(lambda: ax.plot(f, x_range=[a - delta(), a + delta()],
                                              color=C_SEC, stroke_width=7))
        panel_e = live(lambda: eps.get_value(), lambda m: m.move_to(RIGHT * 3.8 + UP * 0.6),
                       "ε = {}", size=38, color=C_EPS)
        panel_d = live(lambda: delta(), lambda m: m.move_to(RIGHT * 3.8 + DOWN * 0.2),
                       "δ = {}", size=38, color=C_H)
        self.add(eb)
        self.play(FadeIn(eb), FadeIn(panel_e))
        self.say("Скептик говорит: «ε = 1». Это розовая полоса: значения "
                 "функции должны попасть в промежуток (L − ε; L + ε).")
        self.add(db)
        self.play(FadeIn(db), FadeIn(panel_d), FadeIn(piece))
        self.say("Мы отвечаем зелёной полосой ширины 2δ вокруг a. Весь жёлтый кусок "
                 "графика над ней лежит внутри розовой полосы — мы выиграли.")
        for e in (0.5, 0.2, 0.06):
            self.play(eps.animate.set_value(e), run_time=2.5)
            self.wait(0.6)
        self.say("Чем строже требование ε, тем уже приходится брать δ. "
                 "Но δ всегда находится — значит, предел действительно равен L.")
        self.play(eps.animate.set_value(0.8), run_time=2)
        self.clear_all()

        # доказательство для 2+h
        head = T("Докажем строго, что lim(2 + h) = 2 при h → 0", 34, C_SEC).to_edge(UP, buff=0.5)
        self.play(Write(head))
        proof = VGroup(
            M(r"\text{Пусть дано } \varepsilon>0.", size=38),
            M(r"|k(h)-2| = |(2+h)-2| = |h|.", size=38),
            M(r"\text{Возьмём } \delta=\varepsilon.", size=38),
            M(r"0<|h|<\delta \ \Rightarrow\ |k(h)-2| = |h| < \delta=\varepsilon.", size=38),
            M(r"\text{Значит, } \lim_{h\to 0}(2+h)=2. \quad \blacksquare", size=38, color=C_LIM),
        ).arrange(DOWN, aligned_edge=LEFT, buff=0.35).next_to(head, DOWN, buff=0.5)
        caps = ["Скептик задаёт любую точность ε.",
                "Оценим, насколько k(h) отличается от 2: ровно на |h|.",
                "Значит, отвечаем δ = ε — этого достаточно.",
                "Если h ближе к нулю, чем δ, то k(h) ближе к 2, чем ε. Игра выиграна при любом ε!",
                "Никаких «бесконечно малых» — только неравенства. В этом сила определения."]
        for p, c in zip(proof, caps):
            self.play(Write(p), run_time=1.3)
            self.say(c)
        self.clear_all()

        # контрпример: скачок
        head = T("Почему у скачка нет предела", 34, C_BAD).to_edge(UP, buff=0.4)
        jx = std_axes([-2, 2, 1], [-1.6, 1.6, 1], 7, 4.2).shift(DOWN * 0.1 + LEFT * 1.8)
        j1 = jx.plot(lambda x: -1, x_range=[-2, 0], color=C_F, stroke_width=4)
        j2 = jx.plot(lambda x: 1, x_range=[0, 2], color=C_F, stroke_width=4)
        jh = VGroup(hole(jx.c2p(0, -1)), hole(jx.c2p(0, 1)))
        self.play(Write(head), Create(jx), Create(j1), Create(j2), FadeIn(jh))
        dd = ValueTracker(1.2)
        eb = band(jx, -2, 2, 0.5, 1.5, C_EPS, 0.2)
        db = always_redraw(lambda: band(jx, -dd.get_value(), dd.get_value(), -1.6, 1.6, C_H, 0.2))
        bad = always_redraw(lambda: Line(jx.c2p(-dd.get_value(), -1), jx.c2p(0, -1),
                                         color=C_BAD, stroke_width=8))
        claim = M(r"L=1?\quad \varepsilon=\tfrac12", size=38).to_edge(RIGHT, buff=0.6).shift(UP * 1)
        self.play(FadeIn(eb), Write(claim))
        self.say("Предположим, что предел равен 1. Скептик берёт ε = 1/2.")
        self.play(FadeIn(db), FadeIn(bad))
        self.say("Какое бы δ мы ни выбрали, в зелёной полосе слева от нуля есть "
                 "точки, где f = −1, — они далеко за пределами розовой полосы.")
        self.play(dd.animate.set_value(0.4), run_time=2)
        self.play(dd.animate.set_value(0.08), run_time=2)
        verdict = VGroup(T("δ не существует", 32, C_BAD), T("⇒ предела нет", 32, C_BAD)).arrange(DOWN, buff=0.15)
        verdict.next_to(claim, DOWN, buff=0.6).to_edge(RIGHT, buff=0.5)
        self.play(Write(verdict))
        self.say("Мы проигрываем при ε = 1/2. Аналогично проигрываем для любого другого L. "
                 "Значит, предела нет — теперь это доказано, а не «видно на глаз».")

    # ═══════════════════════ 6. Производная ══════════════════════════════
    def ch06_derivative(self):
        self.chapter(6, "Определение производной", "предел наклонов секущих")
        s = M(r"\frac{f(x+h)-f(x)}{h}", size=54)
        self.play(Write(s))
        self.say("Возьмём наше разностное отношение — наклон секущей…")
        d = M("f'(x)", "=", r"\lim_{h\to 0}", r"\frac{f(x+h)-f(x)}{h}", size=54)
        d[0].set_color(C_SEC)
        d[2].set_color(C_LIM)
        self.play(TransformMatchingShapes(s, d))
        self.say("…и перейдём к пределу при h → 0. Если этот предел существует, "
                 "он называется ПРОИЗВОДНОЙ функции f в точке x.")
        num = d[3][0:11]
        den = d[3][-1]
        b1 = Brace(num, UP, color=C_DY)
        b1t = T("приращение функции Δy", 24, C_DY).next_to(b1, UP, buff=0.1)
        b2 = Brace(den, DOWN, color=C_H)
        b2t = T("приращение аргумента Δx", 24, C_H).next_to(b2, DOWN, buff=0.1)
        self.play(GrowFromCenter(b1), FadeIn(b1t), GrowFromCenter(b2), FadeIn(b2t))
        self.say("Числитель — изменение функции, знаменатель — изменение аргумента. "
                 "Производная — предел их отношения.")
        self.play(FadeOut(VGroup(b1, b1t, b2, b2t)), d.animate.scale(0.7).to_edge(UP, buff=0.35))
        notes = VGroup(
            M(r"f'(x)", size=40), M(r"y'", size=40), M(r"\frac{df}{dx}", size=40),
            M(r"\frac{dy}{dx}", size=40), M(r"\frac{d}{dx}f(x)", size=40),
        ).arrange(RIGHT, buff=0.9).move_to(UP * 0.8)
        cap = VGroup(T("Лагранж", 24, GREY_B).next_to(notes[0:2], DOWN, buff=0.35),
                     T("Лейбниц", 24, GREY_B).next_to(notes[2:], DOWN, buff=0.35))
        self.play(LaggedStart(*[FadeIn(n, shift=UP * 0.2) for n in notes], lag_ratio=0.2), FadeIn(cap))
        self.say("Обозначения: штрих (Лагранж) или дробь df/dx (Лейбниц). "
                 "dx и df — не числа, а напоминание о пределе Δf/Δx.")
        self.play(FadeOut(notes), FadeOut(cap))
        self.unsay()

        # секущие → касательная
        F = lambda x: 0.2 * x ** 3 - 1.2 * x ** 2 + 2 * x + 1
        Fp = lambda x: 0.6 * x ** 2 - 2.4 * x + 2
        ax = std_axes([0, 5, 1], [0, 6, 1], 6.5, 4.3).shift(LEFT * 3 + DOWN * 0.3)
        graph = ax.plot(F, x_range=[0, 4.95], color=C_F, stroke_width=4)
        self.play(Create(ax), Create(graph))
        x0 = 0.5
        lh = ValueTracker(np.log10(3.5))
        H = lambda: 10 ** lh.get_value()
        sec = always_redraw(lambda: secant(ax, F, x0, H(), length=6,
                                           color=interpolate_color(WHITE, C_SEC, 1 - min(1, H() / 3.5))))
        k = live(lambda: (F(x0 + H()) - F(x0)) / H(), lambda m: m.move_to(RIGHT * 3.6 + UP * 0.8),
                 "наклон = {}", size=36, color=C_SEC)
        hv = live(lambda: H(), lambda m: m.move_to(RIGHT * 3.6 + UP * 1.6), "h = {}", d=4, size=36, color=C_H)
        self.play(FadeIn(sec), FadeIn(k), FadeIn(hv))
        self.play(lh.animate.set_value(-3), run_time=6, rate_func=rate_functions.ease_in_out_sine)
        tl = M(r"f'(0{,}5)=0{,}95", size=38, color=C_SEC).move_to(RIGHT * 3.6 + DOWN * 0.4)
        self.play(Write(tl))
        self.say("Секущие поворачиваются и прижимаются к одной прямой — КАСАТЕЛЬНОЙ. "
                 "Производная — это наклон касательной.")
        self.clear_all()

        # (x^2)' = 2x
        head = M(r"f(x)=x^2", size=44, color=C_F).to_edge(UP, buff=0.4)
        self.play(Write(head))
        self.derive(
            "f'(x)",
            [(r"\lim_{h\to 0}\frac{(x+h)^2-x^2}{h}", "Вычислим производную x² прямо по определению."),
             (r"\lim_{h\to 0}\frac{x^2+2xh+h^2-x^2}{h}", "Раскроем квадрат суммы."),
             (r"\lim_{h\to 0}\frac{2xh+h^2}{h}", "x² взаимно уничтожились."),
             (r"\lim_{h\to 0}\,(2x+h)", "Сократили на h — можно, ведь под пределом h ≠ 0."),
             (r"2x", "Когда h → 0, выражение 2x + h стремится к 2x.")],
            size=36, pos=UP * 0.15, max_h=4.7)
        box = SurroundingRectangle(self.mobjects[-1], color=C_SEC)
        res = M(r"(x^2)'=2x", size=48, color=C_SEC).to_corner(UR, buff=0.5)
        self.play(Write(res))
        self.say("При x = 1 получаем 2 — то самое число, к которому стремилась "
                 "наша таблица. Загадка из начала решена: мгновенная скорость — "
                 "это производная пути.")
        self.clear_all()

        # производная как функция
        top = std_axes([0, 5, 1], [0, 6.5, 2], 8, 2.6, font=20).move_to(UP * 1.85)
        bot = std_axes([0, 5, 1], [-1, 5, 2], 8, 2.2, font=20).move_to(DOWN * 1.45)
        tl1 = M("f(x)", size=32, color=C_F).next_to(top, LEFT, buff=0.3)
        tl2 = M("f'(x)", size=32, color=C_SEC).next_to(bot, LEFT, buff=0.3)
        tg = top.plot(F, x_range=[0, 4.95], color=C_F, stroke_width=4)
        self.play(Create(top), Create(bot), Create(tg), Write(tl1), Write(tl2))
        self.say("Производную можно вычислить в каждой точке — получится новая "
                 "функция f'(x). Посмотрим, как она рождается.")
        x = ValueTracker(0.05)
        tan = always_redraw(lambda: tangent(top, F, Fp, x.get_value(), length=2.4))
        dot = always_redraw(lambda: Dot(top.c2p(x.get_value(), F(x.get_value())), radius=0.06))
        trace = always_redraw(lambda: bot.plot(Fp, x_range=[0.05, max(0.06, x.get_value())],
                                               color=C_SEC, stroke_width=4))
        bdot = always_redraw(lambda: Dot(bot.c2p(x.get_value(), Fp(x.get_value())), color=C_SEC, radius=0.06))
        vline = always_redraw(lambda: DashedLine(top.c2p(x.get_value(), F(x.get_value())),
                                                 bot.c2p(x.get_value(), Fp(x.get_value())),
                                                 color=GREY_C, stroke_width=1.5))
        self.play(FadeIn(tan), FadeIn(dot), FadeIn(trace), FadeIn(bdot), FadeIn(vline))
        self.play(x.animate.set_value(1.183), run_time=3, rate_func=linear)
        self.say("На вершине «горки» касательная горизонтальна — производная равна нулю.")
        self.play(x.animate.set_value(2.816), run_time=3, rate_func=linear)
        self.say("Где функция убывает, касательная смотрит вниз — производная "
                 "отрицательна. В «яме» снова ноль.")
        self.play(x.animate.set_value(4.9), run_time=3, rate_func=linear)
        self.say("Нижний график — производная: он в каждой точке показывает, "
                 "как быстро и в какую сторону меняется верхний.")

    # ═══════════════════════ 7. Когда производной нет ════════════════════
    def ch07_no_derivative(self):
        self.chapter(7, "Когда производной нет", "углы, вертикали, разрывы")

        def window(center, g, title):
            ax = Axes(x_range=[-1, 1, 0.5], y_range=[-1, 1, 0.5], x_length=3.8, y_length=3.8,
                      tips=False, axis_config={"stroke_opacity": 0.4, "color": GREY_B})
            ax.move_to(center)
            frame = Square(3.8, color=GREY_B, stroke_width=2).move_to(center)
            t = M(title, size=32).next_to(frame, UP, buff=0.2)
            return ax, VGroup(ax, frame, t)

        def clipped(ax, g, color):
            us = np.linspace(-1, 1, 301)
            pts = [ax.c2p(u, g(u)) for u in us if abs(g(u)) <= 1]
            return VMobject(color=color, stroke_width=4).set_points_as_corners(pts)

        lw = ValueTracker(0)  # log10 ширины окна
        W = lambda: 10 ** (-lw.get_value())
        axA, winA = window(LEFT * 3.2 + UP * 0.6, None, r"f(x)=x^2,\ \ x=1")
        axB, winB = window(RIGHT * 3.2 + UP * 0.6, None, r"f(x)=|x|,\ \ x=0")
        cA = always_redraw(lambda: clipped(axA, lambda u: ((1 + u * W()) ** 2 - 1) / W(), C_F))
        cB = always_redraw(lambda: clipped(axB, lambda u: abs(u * W()) / W(), C_F2))
        zoom = live(lambda: 1 / W(), lambda m: m.move_to(DOWN * 2.2), "увеличение: ×{}", d=0, size=32)
        self.play(FadeIn(winA), FadeIn(winB), FadeIn(cA), FadeIn(cB), FadeIn(zoom))
        self.say("Производная существует, когда график под лупой выглядит как "
                 "прямая. Будем увеличивать окрестность точки.")
        self.play(lw.animate.set_value(3), run_time=6, rate_func=rate_functions.ease_in_out_sine)
        self.say("Парабола под лупой выпрямляется: это «локальная линейность». "
                 "А у |x| угол остаётся углом при любом увеличении!")
        self.clear_all()

        # 1) угол
        head = T("1. Угол: y = |x|", 36, C_SEC).to_edge(UP, buff=0.35)
        ax = std_axes([-2, 2, 1], [-0.5, 2, 1], 6, 3.8).shift(LEFT * 3.1 + DOWN * 0.4)
        g = ax.plot(np.abs, x_range=[-2, 2], color=C_F, stroke_width=4)
        self.play(Write(head), Create(ax), Create(g))
        form = VGroup(
            M(r"\frac{|0+h|-|0|}{h}=\frac{|h|}{h}", size=38),
            M(r"=\begin{cases}\ \ 1, & h>0\\ -1, & h<0\end{cases}", size=38),
        ).arrange(DOWN, buff=0.3).move_to(RIGHT * 3.4 + UP * 0.9)
        self.play(Write(form[0]))
        self.play(Write(form[1]))
        self.say("Разностное отношение в нуле равно |h|/h: справа это +1, слева −1. "
                 "Односторонние пределы разные — предела нет.")
        ang = ValueTracker(-1)
        def rot_line():
            k = ang.get_value()
            lim = 2 if abs(k) < 0.25 else 0.5 / abs(k)   # не опускаемся ниже y = −0,5
            x0 = -lim if k > 0 else -2
            x1 = lim if k < 0 else 2
            return Line(ax.c2p(x0, k * x0), ax.c2p(x1, k * x1), color=C_SEC, stroke_width=3)

        rot = always_redraw(rot_line)
        self.add(rot)
        self.play(ang.animate.set_value(1), run_time=3)
        self.play(ang.animate.set_value(-0.3), run_time=2)
        self.say("Любая прямая с наклоном от −1 до 1 «касается» угла. "
                 "Раз претендентов много — касательной нет.")
        concl = M(r"f'(0)\ \text{не существует}", size=36, color=C_BAD).next_to(form, DOWN, buff=0.6)
        self.play(Write(concl))
        self.wait(1.5)
        self.clear_all()

        # 2) вертикальная касательная
        head = T("2. Вертикальная касательная: y = ∛x", 36, C_SEC).to_edge(UP, buff=0.35)
        ax = std_axes([-2, 2, 1], [-1.5, 1.5, 1], 6, 3.8).shift(LEFT * 3.1 + DOWN * 0.4)
        g = ax.plot(np.cbrt, x_range=[-2, 2, 0.001], color=C_F, stroke_width=4)
        self.play(Write(head), Create(ax), Create(g))
        lh = ValueTracker(np.log10(1.5))
        Hh = lambda: 10 ** lh.get_value()
        sec = always_redraw(lambda: secant(ax, np.cbrt, 0, Hh(), length=5, legs=False))
        kv = live(lambda: np.cbrt(Hh()) / Hh(), lambda m: m.move_to(RIGHT * 3.4 + DOWN * 0.6),
                  "наклон = {}", d=1, size=34, color=C_SEC)
        form = M(r"\frac{\sqrt[3]{h}}{h}=h^{-2/3}\ \xrightarrow[h\to0]{}\ \infty", size=40).move_to(RIGHT * 3.4 + UP * 0.9)
        self.play(FadeIn(sec), FadeIn(kv), Write(form))
        self.play(lh.animate.set_value(-3), run_time=5)
        self.say("Секущие становятся всё круче, их наклон растёт до бесконечности. "
                 "Касательная вертикальна, а у вертикали нет числового наклона.")
        self.clear_all()

        # 3) разрыв
        head = T("3. Разрыв (скачок)", 36, C_SEC).to_edge(UP, buff=0.35)
        ax = std_axes([-2, 2, 1], [-1.5, 2.5, 1], 6, 3.8).shift(LEFT * 3.1 + DOWN * 0.4)
        J = lambda x: x / 2 if x < 0 else x / 2 + 1
        g1 = ax.plot(lambda x: x / 2, x_range=[-2, -0.001], color=C_F, stroke_width=4)
        g2 = ax.plot(lambda x: x / 2 + 1, x_range=[0, 2], color=C_F, stroke_width=4)
        hh = hole(ax.c2p(0, 0))
        dd = Dot(ax.c2p(0, 1), color=C_F)
        self.play(Write(head), Create(ax), Create(g1), Create(g2), FadeIn(hh), FadeIn(dd))
        lh = ValueTracker(np.log10(1.5))
        Hh = lambda: -(10 ** lh.get_value())
        sec = always_redraw(lambda: secant(ax, J, 0, Hh(), length=5, legs=False))
        self.play(FadeIn(sec))
        self.play(lh.animate.set_value(-2.5), run_time=4)
        txt = VGroup(
            T("Слева секущая перепрыгивает скачок", 28),
            T("и её наклон уходит в бесконечность.", 28),
            M(r"\text{дифференцируема}\ \Rightarrow\ \text{непрерывна}", size=34, color=C_GOOD),
            M(r"\text{непрерывна}\ \not\Rightarrow\ \text{дифференцируема}\ \ (|x|)", size=34, color=C_BAD),
        ).arrange(DOWN, buff=0.3).move_to(RIGHT * 3.3 + UP * 0.2)
        txt.scale_to_fit_width(6.3)
        self.play(FadeIn(txt[:2]))
        self.play(Write(txt[2]))
        self.say("Если функция дифференцируема, то она непрерывна: "
                 "f(x+h) − f(x) = (разностное отношение) · h → f'(x) · 0 = 0.")
        self.play(Write(txt[3]))
        self.say("Обратное неверно: |x| непрерывна, но в нуле производной не имеет.")
        self.clear_all()

        # 4) Вейерштрасс
        head = T("4. Функция Вейерштрасса", 36, C_SEC).to_edge(UP, buff=0.35)
        ax = Axes(x_range=[-1, 1, 0.5], y_range=[-2.2, 2.2, 1], x_length=12, y_length=4.0, tips=False,
                  axis_config={"color": GREY_B, "stroke_opacity": 0.6}).shift(UP * 0.05)
        Wf = lambda x: sum(0.6 ** n * np.cos(3 ** n * np.pi * x) for n in range(7))
        wg = ax.plot(Wf, x_range=[-1, 1, 0.00025], color=C_F, stroke_width=1.6, use_smoothing=False)
        wt = M(r"W(x)=\sum_{n=0}^{\infty} a^n\cos(b^n\pi x),\quad 0<a<1,\ ab\ge 1", size=32).next_to(head, DOWN, buff=0.2)
        self.play(Write(head), Write(wt))
        self.play(Create(ax), Create(wg), run_time=4)
        self.say("Вейерштрасс (1872) построил функцию, непрерывную всюду и не "
                 "дифференцируемую нигде: зигзаги есть на любом масштабе.")
        self.say("Так что производная — не «само собой разумеющееся» свойство. "
                 "Но для привычных функций она есть, и её удобно считать по правилам.")

    # ═══════════════════════ 8. Константа, сумма ═════════════════════════
    def ch08_sum_rule(self):
        self.chapter(8, "Правила: константа и сумма", "каждое — прямо из определения")
        self.say("Каждый раз считать предел долго. Поэтому выведем правила один раз — "
                 "из определения — и дальше будем ими пользоваться.")
        c = M(r"(C)'=\lim_{h\to0}\frac{C-C}{h}=\lim_{h\to0}0=0", size=40).move_to(UP * 2.2)
        xx = M(r"(x)'=\lim_{h\to0}\frac{(x+h)-x}{h}=\lim_{h\to0}1=1", size=40).next_to(c, DOWN, buff=0.5)
        self.play(Write(c))
        self.say("Константа не меняется, её график горизонтален: производная 0.")
        self.play(Write(xx))
        self.say("Прямая y = x растёт с наклоном 1 — производная 1.")
        self.play(FadeOut(c), FadeOut(xx))
        self.unsay()

        self.derive(
            "(f+g)'",
            [(r"\lim_{h\to0}\frac{\big(f(x+h)+g(x+h)\big)-\big(f(x)+g(x)\big)}{h}", None),
             (r"\lim_{h\to0}\left[\frac{f(x+h)-f(x)}{h}+\frac{g(x+h)-g(x)}{h}\right]",
              "Перегруппируем: отдельно приращение f, отдельно приращение g."),
             (r"f'(x)+g'(x)", "Предел суммы равен сумме пределов (это следует из ε–δ).")],
            size=34, pos=UP * 0.8, max_h=4.2)
        self.clear_all()

        # столбики
        base = DOWN * 2.0 + LEFT * 3.5
        fh, gh, df, dg = 1.8, 1.3, 0.45, 0.3
        fb = Rectangle(width=1.4, height=fh).set_fill(C_F, 0.7).set_stroke(width=0)
        fb.move_to(base + UP * fh / 2)
        gb = Rectangle(width=1.4, height=gh).set_fill(C_F2, 0.7).set_stroke(width=0).next_to(fb, UP, buff=0)
        fl = M("f(x)", size=32).next_to(fb, LEFT)
        gl = M("g(x)", size=32).next_to(gb, LEFT)
        self.play(FadeIn(fb), FadeIn(gb), Write(fl), Write(gl))
        self.say("Наглядно: сложим значения f и g в один столбик.")
        dfb = Rectangle(width=1.4, height=df).set_fill(C_H, 0.9).set_stroke(width=0).next_to(fb, UP, buff=0)
        dgb = Rectangle(width=1.4, height=dg).set_fill(C_DY, 0.9).set_stroke(width=0)
        self.play(gb.animate.shift(UP * df), gl.animate.shift(UP * df), FadeIn(dfb))
        dgb.next_to(gb, UP, buff=0)
        self.play(FadeIn(dgb))
        dfl = M("df", size=30, color=C_H).next_to(dfb, RIGHT)
        dgl = M("dg", size=30, color=C_DY).next_to(dgb, RIGHT)
        self.play(Write(dfl), Write(dgl))
        eq = M(r"d(f+g)=df+dg\ \ \Rightarrow\ \ \frac{d(f+g)}{dx}=\frac{df}{dx}+\frac{dg}{dx}", size=38)
        eq.move_to(RIGHT * 2.3 + UP * 0.5)
        self.play(Write(eq))
        self.say("Сдвинем x чуть-чуть: f вырастет на df, g — на dg, "
                 "и весь столбик вырастет на df + dg.")
        cm = M(r"(C\cdot f)'=C\cdot f'", size=44, color=C_SEC).next_to(eq, DOWN, buff=0.8)
        self.play(Write(cm))
        self.say("Точно так же выносится константа: (C·f)' = C·f', потому что "
                 "C выносится за знак предела. Вместе: производная ЛИНЕЙНА.")

    # ═══════════════════════ 9. Степенная функция ════════════════════════
    def ch09_power_rule(self):
        self.chapter(9, "Правило степени", r"почему (xⁿ)' = n·xⁿ⁻¹")
        S = 2.6
        corner = LEFT * 5.6 + DOWN * 1.7
        sq = Square(S).set_fill(C_F, 0.55).set_stroke(WHITE, 2).move_to(corner + (UP + RIGHT) * S / 2)
        bx = Brace(sq, DOWN)
        bxl = M("x", size=34).next_to(bx, DOWN, buff=0.1)
        by = Brace(sq, LEFT)
        byl = M("x", size=34).next_to(by, LEFT, buff=0.1)
        area = M("x^2", size=40).move_to(sq)
        self.play(DrawBorderThenFill(sq), GrowFromCenter(bx), GrowFromCenter(by), Write(bxl), Write(byl), Write(area))
        self.say("Представим x² как площадь квадрата со стороной x.")
        h = ValueTracker(0.7)
        rs = always_redraw(lambda: Rectangle(width=h.get_value(), height=S).set_fill(C_H, 0.75)
                           .set_stroke(WHITE, 1).next_to(sq, RIGHT, buff=0))
        ts = always_redraw(lambda: Rectangle(width=S, height=h.get_value()).set_fill(C_H, 0.75)
                           .set_stroke(WHITE, 1).next_to(sq, UP, buff=0))
        cs = always_redraw(lambda: Square(h.get_value()).set_fill(C_DY, 0.9).set_stroke(WHITE, 1)
                           .move_to(sq.get_corner(UR) + (UP + RIGHT) * h.get_value() / 2))
        hl = always_redraw(lambda: M("h", size=30, color=C_H).next_to(rs, DOWN, buff=0.15))
        self.play(FadeIn(rs), FadeIn(ts), FadeIn(cs), FadeIn(hl))
        lab1 = M("xh", size=30).move_to(RIGHT * 0.1 + UP * 2.8)
        self.say("Увеличим сторону на h. Прибавилось: две полоски x·h (зелёные) "
                 "и маленький квадратик h² (красный).")
        eqs = VGroup(
            M(r"(x+h)^2-x^2", "=", r"2xh", "+", "h^2", size=40),
            M(r"\frac{(x+h)^2-x^2}{h}", "=", r"2x", "+", "h", size=40),
            M(r"(x^2)'", "=", r"2x", size=44),
        ).arrange(DOWN, buff=0.5).move_to(RIGHT * 3 + UP * 0.6)
        eqs[0][2].set_color(C_H)
        eqs[0][4].set_color(C_DY)
        eqs[2].set_color(C_SEC)
        self.play(Write(eqs[0]))
        self.play(h.animate.set_value(0.12), run_time=3)
        self.say("Когда h мало, квадратик h² исчезающе мал даже по сравнению "
                 "с полосками: он «второго порядка малости».")
        self.play(Write(eqs[1]))
        self.play(Write(eqs[2]))
        self.say("Делим на h и устремляем h к нулю — остаётся 2x: две полоски длины x.")
        self.clear_all()

        head = T("Куб и общий случай", 36, C_SEC).to_edge(UP, buff=0.4)
        cube = M(r"(x+h)^3=x^3+", r"3x^2h", r"+3xh^2+h^3", size=40).next_to(head, DOWN, buff=0.5)
        cube[1].set_color(C_H)
        self.play(Write(head), Write(cube))
        self.say("Для куба: объём x³ прирастает тремя «плитками» x²·h и мелочью "
                 "с h², h³. Производная 3x².")
        binom = M(r"(x+h)^n", "=", "x^n", "+", r"n\,x^{n-1}h", "+",
                  r"\tbinom{n}{2}x^{n-2}h^2+\dots+h^n", size=38).next_to(cube, DOWN, buff=0.6)
        binom[4].set_color(C_H)
        binom[6].set_color(GREY_B)
        self.play(Write(binom))
        br = Brace(binom[6], DOWN, color=GREY_B)
        brt = T("всё содержит h² и выше", 24, GREY_B).next_to(br, DOWN, buff=0.1)
        self.play(GrowFromCenter(br), FadeIn(brt))
        self.say("Бином Ньютона: всё, кроме первых двух слагаемых, содержит h² или "
                 "выше. После деления на h эти члены всё ещё содержат h и исчезают.")
        res = M(r"\frac{(x+h)^n-x^n}{h}=n\,x^{n-1}+h\cdot(\dots)\ \xrightarrow[h\to0]{}\ n\,x^{n-1}",
                size=38).next_to(brt, DOWN, buff=0.45)
        self.play(Write(res))
        fin = M(r"(x^n)'=n\,x^{n-1}", size=52, color=C_SEC).next_to(res, DOWN, buff=0.45)
        self.play(Write(fin))
        self.play(Circumscribe(fin, color=C_SEC))
        self.say("Правило степени! Показатель спускается вниз множителем, а "
                 "степень уменьшается на единицу.")
        self.play(FadeOut(VGroup(cube, binom, br, brt)),
                  VGroup(res, fin).animate.next_to(head, DOWN, buff=0.6))
        ex = M(r"(x^5)'=5x^4,\qquad \left(\frac{1}{x}\right)'=-\frac{1}{x^2},\qquad (\sqrt{x})'=\frac{1}{2\sqrt{x}}",
               size=36)
        ex.next_to(fin, DOWN, buff=0.4)
        self.play(Write(ex))
        self.say("Формула верна для любых показателей, даже дробных и отрицательных — "
                 "это мы докажем позже, с помощью логарифма.")

    # ═══════════════════════ 10. Произведение ════════════════════════════
    def ch10_product_rule(self):
        self.chapter(10, "Правило произведения", "(f·g)' = f'g + fg'")
        W0, H0 = 3.8, 2.4
        corner = LEFT * 6 + DOWN * 1.7
        rect = Rectangle(width=W0, height=H0).set_fill(C_F, 0.5).set_stroke(WHITE, 2)
        rect.move_to(corner + RIGHT * W0 / 2 + UP * H0 / 2)
        fb = Brace(rect, DOWN)
        fl = M("f(x)", size=32).next_to(fb, DOWN, buff=0.1)
        gb = Brace(rect, LEFT)
        gl = M("g(x)", size=32).next_to(gb, LEFT, buff=0.1)
        al = M(r"f\cdot g", size=40).move_to(rect)
        self.play(DrawBorderThenFill(rect), GrowFromCenter(fb), GrowFromCenter(gb), Write(fl), Write(gl), Write(al))
        self.say("Произведение f·g — это площадь прямоугольника со сторонами f(x) и g(x).")
        dfv, dgv = ValueTracker(0.9), ValueTracker(0.6)
        right = always_redraw(lambda: Rectangle(width=dfv.get_value(), height=H0).set_fill(C_H, 0.75)
                              .set_stroke(WHITE, 1).next_to(rect, RIGHT, buff=0))
        top = always_redraw(lambda: Rectangle(width=W0, height=dgv.get_value()).set_fill(C_SEC, 0.6)
                            .set_stroke(WHITE, 1).next_to(rect, UP, buff=0))
        cor = always_redraw(lambda: Rectangle(width=dfv.get_value(), height=dgv.get_value())
                            .set_fill(C_DY, 0.9).set_stroke(WHITE, 1)
                            .move_to(rect.get_corner(UR) + RIGHT * dfv.get_value() / 2 + UP * dgv.get_value() / 2))
        rl = always_redraw(lambda: M(r"g\,df", size=28).rotate(PI / 2).move_to(right))
        tl = always_redraw(lambda: M(r"f\,dg", size=28).move_to(top))
        self.play(FadeIn(right), FadeIn(top), FadeIn(cor), FadeIn(rl), FadeIn(tl))
        self.say("Чуть сдвинем x: f вырастет на df, g — на dg. Площадь прибавится "
                 "на полоску g·df справа, полоску f·dg сверху и уголок df·dg.")
        eq = M(r"d(fg)=", r"g\,df", "+", r"f\,dg", "+", r"df\,dg", size=40).move_to(RIGHT * 2.8 + UP * 2.2)
        eq[1].set_color(C_H)
        eq[3].set_color(C_SEC)
        eq[5].set_color(C_DY)
        self.play(Write(eq))
        self.play(dfv.animate.set_value(0.25), dgv.animate.set_value(0.16), run_time=3)
        self.say("Уголок df·dg — произведение двух малых величин, при делении "
                 "на dx он всё равно стремится к нулю.")
        rule = M(r"(fg)'=f'g+fg'", size=48, color=C_SEC).next_to(eq, DOWN, buff=0.7)
        self.play(Write(rule))
        self.play(Circumscribe(rule, color=C_SEC))
        self.say("Остаётся правило произведения: производная первого на второе плюс "
                 "первое на производную второго.")
        self.clear_all()

        head = T("Строгий вывод: прибавим и вычтем f(x+h)·g(x)", 32, C_SEC).to_edge(UP, buff=0.35)
        self.play(Write(head))
        self.derive(
            r"\frac{f(x+h)g(x+h)-f(x)g(x)}{h}",
            [(r"\frac{f(x+h)g(x+h)-f(x+h)g(x)+f(x+h)g(x)-f(x)g(x)}{h}",
              "Хитрость: добавим ноль в виде −f(x+h)g(x) + f(x+h)g(x)."),
             (r"f(x+h)\cdot\frac{g(x+h)-g(x)}{h}+g(x)\cdot\frac{f(x+h)-f(x)}{h}",
              "Сгруппируем и вынесем общие множители."),
             (r"\ \xrightarrow[h\to0]{}\ f(x)\,g'(x)+g(x)\,f'(x)",
              "f(x+h) → f(x), потому что дифференцируемая функция непрерывна (глава 7).")],
            size=32, pos=DOWN * 0.1, max_h=4.6)
        chk = M(r"\text{Проверка: } (x^2\cdot x^3)'=2x\cdot x^3+x^2\cdot 3x^2=5x^4=(x^5)'\ \checkmark",
                size=34, color=C_GOOD).next_to(head, DOWN, buff=0.15)
        self.play(Write(chk))
        self.say("Проверка: по правилу произведения (x²·x³)' = 5x⁴, и по правилу "
                 "степени (x⁵)' = 5x⁴. Всё сходится.")

    # ═══════════════════════ 11. Цепное правило ══════════════════════════
    def ch11_chain_rule(self):
        self.chapter(11, "Цепное правило", "производная сложной функции")
        self.say("Как дифференцировать f(g(x)) — функцию от функции? "
                 "Например, (x²)³. Проследим за маленьким толчком dx.")
        rng = [0.8, 1.6, 0.1]
        mk = lambda y: NumberLine(x_range=rng, length=10, include_numbers=False, color=GREY_B).shift(UP * y + RIGHT * 0.8)
        Lx, Lu, Ly = mk(2.7), mk(1.0), mk(-0.7)
        for L in (Lx, Lu, Ly):
            L.add_labels({1: M("1", size=24), 1.5: M("1{,}5", size=24)})
        nx = M("x", size=36).next_to(Lx, LEFT, buff=0.4)
        nu = M(r"u=g(x)=x^2", size=32).next_to(Lu, LEFT, buff=0.3)
        ny = M(r"y=f(u)=u^3", size=32).next_to(Ly, LEFT, buff=0.3)
        for m in (nu, ny):
            if m.get_left()[0] < -7:
                m.shift(RIGHT * (-6.9 - m.get_left()[0]))
        self.play(Create(Lx), Create(Lu), Create(Ly), Write(nx), Write(nu), Write(ny))
        dx = 0.05
        seg = lambda L, a, b, c: Line(L.n2p(a), L.n2p(b), color=c, stroke_width=10)
        sx = seg(Lx, 1, 1 + dx, C_H)
        su = seg(Lu, 1, (1 + dx) ** 2, C_SEC)
        sy = seg(Ly, 1, (1 + dx) ** 6, C_DY)
        lx = M("dx", size=30, color=C_H).next_to(sx, UP, buff=0.15)
        lu = M("du", size=30, color=C_SEC).next_to(su, UP, buff=0.15)
        ly = M("dy", size=30, color=C_DY).next_to(sy, UP, buff=0.15)
        self.play(Create(sx), Write(lx))
        self.say("Толкнём x = 1 на dx = 0,05 (зелёный отрезок).")
        a1 = VGroup(Arrow(Lx.n2p(1), Lu.n2p(1), buff=0.1, color=GREY_B, stroke_width=2),
                    Arrow(Lx.n2p(1 + dx), Lu.n2p((1 + dx) ** 2), buff=0.1, color=GREY_B, stroke_width=2))
        self.play(GrowArrow(a1[0]), GrowArrow(a1[1]))
        self.play(Create(su), Write(lu))
        k1 = M(r"\times\, g'(x)=\times 2", size=32, color=C_SEC).next_to(a1, RIGHT, buff=0.6)
        self.play(Write(k1))
        self.say("Функция g(x) = x² растягивает толчок примерно в g'(1) = 2 раза: du ≈ 2·dx.")
        a2 = VGroup(Arrow(Lu.n2p(1), Ly.n2p(1), buff=0.1, color=GREY_B, stroke_width=2),
                    Arrow(Lu.n2p((1 + dx) ** 2), Ly.n2p((1 + dx) ** 6), buff=0.1, color=GREY_B, stroke_width=2))
        self.play(GrowArrow(a2[0]), GrowArrow(a2[1]))
        self.play(Create(sy), Write(ly))
        k2 = M(r"\times\, f'(u)=\times 3", size=32, color=C_DY).next_to(a2, RIGHT, buff=1.5)
        self.play(Write(k2))
        self.say("Затем f(u) = u³ растягивает du ещё в f'(1) = 3 раза. Итого толчок "
                 "вырос в 2 · 3 = 6 раз.")
        tot = M(r"\frac{dy}{dx}=\frac{dy}{du}\cdot\frac{du}{dx}=3\cdot 2=6=(x^6)'\big|_{x=1}\ \checkmark",
                size=34).move_to(DOWN * 1.95)
        self.play(Write(tot))
        self.say("И действительно, (x²)³ = x⁶, а (x⁶)' = 6x⁵ = 6 при x = 1. "
                 "Растяжения ПЕРЕМНОЖАЮТСЯ.")
        self.clear_all()

        rule = M(r"\big(f(g(x))\big)'=f'\big(g(x)\big)\cdot g'(x)", size=48, color=C_SEC).to_edge(UP, buff=0.4)
        self.play(Write(rule))
        chain = self.derive(
            r"\frac{f(g(x+h))-f(g(x))}{h}",
            [(r"\frac{f(g(x+h))-f(g(x))}{g(x+h)-g(x)}\cdot\frac{g(x+h)-g(x)}{h}",
              "Умножим и разделим на Δu = g(x+h) − g(x)."),
             (r"\frac{f(u+\Delta u)-f(u)}{\Delta u}\cdot\frac{g(x+h)-g(x)}{h}",
              "Первая дробь — разностное отношение для f в точке u = g(x)."),
             (r"\ \xrightarrow[h\to0]{}\ f'(u)\cdot g'(x)",
              "При h → 0 и Δu → 0 (g непрерывна) — получаем произведение производных.")],
            size=34, pos=DOWN * 0.3, max_h=4.4)
        self.say("Тонкость: если Δu = 0 при сколь угодно малых h, делить на Δu нельзя; "
                 "аккуратное доказательство обходит это вспомогательной функцией.")
        self.play(FadeOut(chain))
        ex = M(r"\big((x^2+1)^3\big)'=3(x^2+1)^2\cdot 2x", size=48, color=C_GOOD).move_to(UP * 0.5)
        self.play(Write(ex))
        self.say("Пример: внешняя функция — куб, внутренняя — x² + 1. "
                 "Производная внешней в точке внутренней, умноженная на производную внутренней.")

    # ═══════════════════════ 12. Экспонента ══════════════════════════════
    def ch12_exponential(self):
        self.chapter(12, "Экспонента eˣ", "функция, равная своей производной")
        self.derive(
            r"(a^x)'",
            [(r"\lim_{h\to0}\frac{a^{x+h}-a^x}{h}", None),
             (r"\lim_{h\to0}\frac{a^x\,a^h-a^x}{h}", "Свойство степеней: a^(x+h) = a^x · a^h."),
             (r"a^x\cdot\lim_{h\to0}\frac{a^h-1}{h}", "a^x не зависит от h — выносим его за предел."),
             (r"a^x\cdot M(a)", "Производная aˣ ПРОПОРЦИОНАЛЬНА самой функции! "
                                "Коэффициент M(a) — предел, зависящий только от a.")],
            size=36, pos=UP * 0.3, max_h=5)
        self.clear_all()

        ax = std_axes([-3, 3, 1], [0, 8, 2], 6.4, 4.4).shift(LEFT * 3 + UP * 0.05)
        g2 = ax.plot(lambda x: 2 ** x, x_range=[-3, 2.95], color=C_F, stroke_width=4)
        gl = M("y=2^x", size=34, color=C_F).next_to(ax.c2p(2.9, 7.5), LEFT)
        self.play(Create(ax), Create(g2), Write(gl))
        x = ValueTracker(-1.5)
        ln2 = np.log(2)
        tan = always_redraw(lambda: tangent(ax, lambda t: 2 ** t, lambda t: ln2 * 2 ** t, x.get_value(), 3))
        hgt = always_redraw(lambda: Line(ax.c2p(x.get_value(), 0), ax.c2p(x.get_value(), 2 ** x.get_value()),
                                         color=C_DY, stroke_width=4))
        pos = lambda k: (lambda m: m.move_to(RIGHT * 3.6 + UP * (1.6 - 0.8 * k)))
        v1 = live(lambda: 2 ** x.get_value(), pos(0), "высота = {}", size=32, color=C_DY)
        v2 = live(lambda: ln2 * 2 ** x.get_value(), pos(1), "наклон = {}", size=32, color=C_SEC)
        v3 = live(lambda: ln2, pos(2), "наклон / высота = {}", size=32, color=C_GOOD)
        self.play(FadeIn(tan), FadeIn(hgt), FadeIn(v1), FadeIn(v2), FadeIn(v3))
        self.play(x.animate.set_value(2.5), run_time=5, rate_func=linear)
        self.say("Для 2ˣ отношение «наклон / высота» всегда одно и то же: ≈ 0,693. "
                 "Это и есть M(2).")
        self.clear_all()

        ax = std_axes([-2, 2, 1], [0, 8, 2], 6.4, 4.4).shift(LEFT * 3 + UP * 0.05)
        a = ValueTracker(2)
        gr = always_redraw(lambda: ax.plot(lambda t: a.get_value() ** t,
                                           x_range=[-2, np.log(8) / np.log(a.get_value()) - 0.01],
                                           color=C_F, stroke_width=4))
        tan0 = always_redraw(lambda: tangent(ax, lambda t: a.get_value() ** t,
                                             lambda t: np.log(a.get_value()) * a.get_value() ** t, 0, 4.5))
        ref = DashedLine(ax.c2p(-1, 0), ax.c2p(2, 3), color=GREY_B)
        refl = M("y=x+1", size=28, color=GREY_B).next_to(ax.c2p(2, 3), UP, buff=0.1)
        av = live(lambda: a.get_value(), lambda m: m.move_to(RIGHT * 3.6 + UP * 1.4), "a = {}", d=4, size=38)
        mv = live(lambda: np.log(a.get_value()), lambda m: m.move_to(RIGHT * 3.6 + UP * 0.5),
                  "M(a) = {}", d=4, size=38, color=C_SEC)
        self.play(Create(ax), FadeIn(gr), FadeIn(tan0), FadeIn(av), FadeIn(mv))
        self.say("M(a) — это наклон графика aˣ в точке x = 0. Для a = 2 он ≈ 0,693, "
                 "меньше единицы. Попробуем a = 3.")
        self.play(a.animate.set_value(3), run_time=3)
        self.say("Для a = 3 наклон ≈ 1,099 — больше единицы. Значит, где-то между "
                 "2 и 3 есть «идеальное» основание с M(a) = 1.")
        self.play(Create(ref), Write(refl))
        self.play(a.animate.set_value(np.e), run_time=3)
        e_t = M(r"e\approx 2{,}71828\ldots", size=44, color=C_SEC).move_to(RIGHT * 3.6 + DOWN * 0.6)
        self.play(Write(e_t))
        self.say("Это основание — число e ≈ 2,71828. Касательная в нуле совпадает "
                 "с прямой y = x + 1.")
        lim = M(r"e=\lim_{n\to\infty}\left(1+\frac1n\right)^n", size=36).next_to(e_t, DOWN, buff=0.4)
        self.play(Write(lim))
        self.say("То же число возникает как предел (1 + 1/n)ⁿ: при n = 10 это 2,594, "
                 "при n = 1000 — 2,717, при n = 10⁶ — 2,71828.")
        self.clear_all()

        res = M(r"(e^x)'=e^x", size=64, color=C_SEC).to_edge(UP, buff=0.5)
        self.play(Write(res))
        ax = std_axes([-2, 2, 1], [0, 7, 1], 6.4, 3.9).shift(DOWN * 0.45)
        ge = ax.plot(np.exp, x_range=[-2, np.log(7) - 0.01], color=C_F, stroke_width=4)
        self.play(Create(ax), Create(ge))
        x = ValueTracker(0.3)

        def tri():
            xv = x.get_value()
            p, c, q = ax.c2p(xv, np.exp(xv)), ax.c2p(xv + 1, np.exp(xv)), ax.c2p(xv + 1, 2 * np.exp(xv))
            h0 = ax.c2p(xv, 0)
            return VGroup(Line(h0, p, color=C_DY, stroke_width=5),
                          Line(p, c, color=C_H, stroke_width=4), Line(c, q, color=C_DY, stroke_width=5),
                          tangent(ax, np.exp, np.exp, xv, 5), Dot(p))

        tg = always_redraw(tri)
        self.play(FadeIn(tg))
        self.say("Смысл: сдвинься вправо на 1 по касательной — поднимешься ровно на "
                 "высоту самой точки. Скорость роста равна текущему значению.")
        self.play(x.animate.set_value(-1.5), run_time=3)
        self.play(x.animate.set_value(0.8), run_time=3)
        self.say("Поэтому eˣ описывает всё, что растёт пропорционально себе: "
                 "деньги под проценты, размножение бактерий, цепные реакции.")

    # ═══════════════════════ 13. Логарифм ════════════════════════════════
    def ch13_logarithm(self):
        self.chapter(13, "Логарифм ln x", "обратная функция и зеркало")
        ax = Axes(x_range=[-2.5, 5, 1], y_range=[-2.5, 5, 1], x_length=5.5, y_length=5.5, tips=False,
                  axis_config={"color": GREY_B, "include_numbers": True, "font_size": 20})
        ax.shift(LEFT * 3.4 + UP * 0.25)
        ge = ax.plot(np.exp, x_range=[-2.5, np.log(5) - 0.001], color=C_F, stroke_width=4)
        gl = ax.plot(np.log, x_range=[np.exp(-2.5), 5], color=C_F2, stroke_width=4)
        diag = DashedLine(ax.c2p(-2.5, -2.5), ax.c2p(5, 5), color=GREY_B)
        le = M("e^x", size=32, color=C_F).next_to(ax.c2p(1.5, 4.5), LEFT)
        ll = M(r"\ln x", size=32, color=C_F2).next_to(ax.c2p(4.5, 1.5), DOWN)
        self.play(Create(ax), Create(ge), Write(le))
        self.play(Create(diag))
        self.play(TransformFromCopy(ge, gl), Write(ll), run_time=2)
        self.say("ln x — функция, обратная к eˣ: если y = ln x, то x = e^y. "
                 "Её график — зеркальное отражение графика eˣ относительно y = x.")
        t = ValueTracker(-0.8)
        tp = always_redraw(lambda: tangent(ax, np.exp, np.exp, t.get_value(), 2.6))
        tq = always_redraw(lambda: tangent(ax, np.log, lambda s: 1 / s, np.exp(t.get_value()), 2.6, color=C_EPS))
        dp = always_redraw(lambda: Dot(ax.c2p(t.get_value(), np.exp(t.get_value()))))
        dq = always_redraw(lambda: Dot(ax.c2p(np.exp(t.get_value()), t.get_value())))
        p = lambda k: (lambda m: m.move_to(RIGHT * 3.3 + UP * (2.2 - 0.75 * k)))
        v1 = live(lambda: np.exp(t.get_value()), p(0), "наклон eˣ: {}", size=30, color=C_SEC)
        v2 = live(lambda: np.exp(-t.get_value()), p(1), "наклон ln x: {}", size=30, color=C_EPS)
        v3 = live(lambda: np.exp(t.get_value()), p(2), "x = {}", size=30, color=C_F2)
        self.play(FadeIn(tp), FadeIn(tq), FadeIn(dp), FadeIn(dq), FadeIn(v1), FadeIn(v2), FadeIn(v3))
        self.play(t.animate.set_value(1.2), run_time=4)
        self.play(t.animate.set_value(0.3), run_time=2)
        self.say("При отражении подъём и пробег меняются местами, поэтому наклоны "
                 "взаимно обратны. Наклон ln x в точке x равен 1/x.")
        self.clear_all()

        head = T("Два вывода", 36, C_SEC).to_edge(UP, buff=0.3)
        self.play(Write(head))
        A = VGroup(
            T("1) Через цепное правило", 28, GREY_A),
            M(r"e^{\ln x}=x", size=36),
            M(r"e^{\ln x}\cdot(\ln x)'=1", size=36),
            M(r"x\cdot(\ln x)'=1", size=36),
            M(r"(\ln x)'=\frac1x", size=40, color=C_SEC),
        ).arrange(DOWN, buff=0.3).move_to(LEFT * 3.5 + DOWN * 0.3)
        for m in A:
            self.play(Write(m), run_time=0.9)
        self.say("Продифференцируем тождество e^(ln x) = x по цепному правилу — "
                 "и сразу получаем (ln x)' = 1/x.")
        B = VGroup(
            T("2) Прямо по определению", 28, GREY_A),
            M(r"\frac{\ln(x+h)-\ln x}{h}=\frac1h\ln\!\left(1+\frac hx\right)", size=32),
            M(r"=\frac1x\ln\!\left(1+\frac hx\right)^{x/h}", size=32),
            M(r"\xrightarrow[h\to0]{}\ \frac1x\ln e=\frac1x", size=34, color=C_SEC),
        ).arrange(DOWN, buff=0.35).move_to(RIGHT * 3.4 + DOWN * 0.3)
        for m in B:
            self.play(Write(m), run_time=1.1)
        self.say("Или по определению: обозначив n = x/h, видим под логарифмом "
                 "(1 + 1/n)ⁿ → e. А ln e = 1.")
        self.clear_all()

        head = T("Следствия", 36, C_SEC).to_edge(UP, buff=0.35)
        self.play(Write(head))
        c1 = M(r"a^x=e^{x\ln a}\ \Rightarrow\ (a^x)'=e^{x\ln a}\cdot\ln a=a^x\ln a", size=36)
        c1b = M(r"M(2)=\ln 2\approx 0{,}693\ \checkmark", size=34, color=C_GOOD)
        c2 = M(r"x^n=e^{n\ln x}\ \Rightarrow\ (x^n)'=e^{n\ln x}\cdot\frac nx=n\,x^{n-1}\quad(n\in\mathbb R)", size=36)
        c3 = M(r"\left(\frac fg\right)'=\big(f\cdot g^{-1}\big)'=f'g^{-1}-f\,g^{-2}g'=\frac{f'g-fg'}{g^2}", size=36)
        grp = VGroup(c1, c1b, c2, c3).arrange(DOWN, buff=0.45).next_to(head, DOWN, buff=0.5)
        self.play(Write(c1))
        self.play(Write(c1b))
        self.say("Загадочное число 0,693 из главы про экспоненту — это ln 2!")
        self.play(Write(c2))
        self.say("И правило степени теперь доказано для ЛЮБОГО вещественного показателя.")
        self.play(Write(c3))
        self.say("А из правил произведения, степени и цепочки сразу следует "
                 "правило частного.")
        self.clear_all()

        head = T("Шпаргалка: всё выведено из определения", 36, C_SEC).to_edge(UP, buff=0.35)
        rules = [
            r"(C)'=0", r"(f+g)'=f'+g'", r"(Cf)'=Cf'",
            r"(x^n)'=n\,x^{n-1}", r"(fg)'=f'g+fg'", r"\left(\tfrac fg\right)'=\tfrac{f'g-fg'}{g^2}",
            r"\big(f(g)\big)'=f'(g)\,g'", r"(e^x)'=e^x", r"(\ln x)'=\tfrac1x",
        ]
        cells = VGroup(*[M(r, size=38) for r in rules]).arrange_in_grid(rows=3, cols=3, buff=(0.8, 0.8))
        cells.scale_to_fit_width(12.5).next_to(head, DOWN, buff=0.7)
        boxes = VGroup(*[SurroundingRectangle(c, buff=0.2, corner_radius=0.1, color=GREY_B, stroke_width=1.5)
                         for c in cells])
        self.play(Write(head))
        self.play(LaggedStart(*[AnimationGroup(Create(b), Write(c)) for b, c in zip(boxes, cells)],
                              lag_ratio=0.3), run_time=5)
        self.say("Девять правил — и ни одно не надо заучивать вслепую: за каждым "
                 "стоит один и тот же предел разностного отношения.")

    # ═══════════════════════ 14. Применения ══════════════════════════════
    def ch14_applications(self):
        self.chapter(14, "Применения", "скорость, оптимум, приближения")
        # 1) падение
        top_y, scale = 2.6, 0.9
        pole = Line(UP * top_y + LEFT * 5, UP * (top_y - 4.9 * scale) + LEFT * 5, color=GREY_C)
        ticks = VGroup(*[VGroup(Line(LEFT * 0.12, RIGHT * 0.12, color=GREY_C),
                                M(f"{k}\\,\\text{{м}}", size=22).shift(LEFT * 0.55))
                         .move_to(LEFT * 5 + UP * (top_y - k * scale)) for k in range(0, 5)])
        t = ValueTracker(0)
        ball = always_redraw(lambda: Dot(LEFT * 4.4 + UP * (top_y - 4.9 * t.get_value() ** 2 * scale),
                                         radius=0.16, color=C_SEC))
        vel = always_redraw(lambda: Arrow(ball.get_center(), ball.get_center() + DOWN * (0.02 + 0.13 * 9.8 * t.get_value()),
                                          buff=0, color=C_DY, stroke_width=5, max_tip_length_to_length_ratio=0.25))
        self.play(Create(pole), FadeIn(ticks), FadeIn(ball))
        f1 = VGroup(M(r"s(t)=4{,}9\,t^2", size=40),
                    M(r"v(t)=s'(t)=9{,}8\,t", size=40, color=C_DY),
                    M(r"a(t)=v'(t)=9{,}8\ \text{м/с}^2", size=40, color=C_SEC)).arrange(DOWN, aligned_edge=LEFT, buff=0.4)
        f1.move_to(RIGHT * 2.2 + UP * 1.4)
        self.play(Write(f1[0]))
        self.say("Камень падает: за время t он пролетает s = 4,9t² метров (без сопротивления воздуха).")
        self.play(Write(f1[1]))
        self.say("Скорость — производная пути: по правилу степени v = 9,8t. Это и есть "
                 "показание «спидометра» в каждый момент.")
        self.add(vel)
        tv = live(lambda: t.get_value(), lambda m: m.move_to(RIGHT * 2.2 + DOWN * 0.9), "t = {} с", d=2, size=32)
        vv = live(lambda: 9.8 * t.get_value(), lambda m: m.move_to(RIGHT * 2.2 + DOWN * 1.6),
                  "v = {} м/с", d=2, size=32, color=C_DY)
        self.play(FadeIn(tv), FadeIn(vv))
        self.play(t.animate.set_value(1), run_time=4, rate_func=linear)
        self.play(Write(f1[2]))
        self.say("Производная скорости — ускорение: постоянное 9,8 м/с². "
                 "Это ускорение свободного падения g.")
        self.clear_all()

        # 2) забор
        head = T("Задача: 20 м забора. Какой прямоугольник даст наибольшую площадь?", 30, C_SEC)
        head.scale_to_fit_width(min(head.width, 13)).to_edge(UP, buff=0.35)
        self.play(Write(head))
        x = ValueTracker(1.5)
        k = 0.34
        rect = always_redraw(lambda: Rectangle(width=x.get_value() * k, height=(10 - x.get_value()) * k)
                             .set_fill(C_F, 0.4).set_stroke(C_F, 3).move_to(LEFT * 4.6 + DOWN * 0.3))
        rx = always_redraw(lambda: M("x", size=30).next_to(rect, DOWN, buff=0.12))
        ry = always_redraw(lambda: M("10-x", size=30).next_to(rect, LEFT, buff=0.12))
        ax = std_axes([0, 10, 2], [0, 30, 10], 5.2, 3.6, font=20).shift(RIGHT * 2.0 + UP * 0.2)
        S = lambda v: v * (10 - v)
        Sg = ax.plot(S, x_range=[0, 10], color=C_SEC, stroke_width=4)
        tn = always_redraw(lambda: tangent(ax, S, lambda v: 10 - 2 * v, x.get_value(), 2.4, color=C_EPS))
        dt = always_redraw(lambda: Dot(ax.c2p(x.get_value(), S(x.get_value()))))
        sv = live(lambda: S(x.get_value()), lambda m: m.next_to(ax, UP, buff=0.1).shift(LEFT * 1.3),
                  "S = {} м²", d=2, size=28)
        dv = live(lambda: 10 - 2 * x.get_value(), lambda m: m.next_to(ax, UP, buff=0.1).shift(RIGHT * 1.5),
                  "S' = {}", d=2, size=28, color=C_EPS)
        self.play(FadeIn(rect), FadeIn(rx), FadeIn(ry), Create(ax), Create(Sg), FadeIn(tn), FadeIn(dt),
                  FadeIn(sv), FadeIn(dv))
        self.say("Если одна сторона x, то другая 10 − x, и площадь S(x) = x(10 − x) = 10x − x².")
        self.play(x.animate.set_value(8.5), run_time=4)
        self.play(x.animate.set_value(5), run_time=3)
        sol = M(r"S'(x)=10-2x=0\ \Rightarrow\ x=5,\quad S_{\max}=25\ \text{м}^2", size=36, color=C_GOOD)
        sol.move_to(DOWN * 2.3)
        self.play(Write(sol))
        self.say("В вершине касательная горизонтальна: S'(x) = 0. Отсюда x = 5 — "
                 "лучший прямоугольник оказывается квадратом.")
        self.clear_all()

        # 3) линейное приближение
        head = T("Линейное приближение: посчитаем √4,1 без калькулятора", 32, C_SEC).to_edge(UP, buff=0.35)
        self.play(Write(head))
        ax = std_axes([0, 7, 1], [0, 3, 1], 6.2, 3.8).shift(LEFT * 3.1 + DOWN * 0.1)
        g = ax.plot(np.sqrt, x_range=[0, 7, 0.01], color=C_F, stroke_width=4)
        tl = ax.plot(lambda v: 2 + (v - 4) / 4, x_range=[0.5, 7], color=C_SEC, stroke_width=3)
        d4 = Dot(ax.c2p(4, 2))
        self.play(Create(ax), Create(g))
        self.play(Create(tl), FadeIn(d4))
        eqs = VGroup(
            M(r"f(a+h)\approx f(a)+f'(a)\,h", size=38, color=C_SEC),
            M(r"f(x)=\sqrt x,\quad f'(x)=\frac{1}{2\sqrt x}", size=34),
            M(r"\sqrt{4{,}1}\approx 2+\frac{1}{4}\cdot 0{,}1=2{,}025", size=36),
            M(r"\text{точно: } 2{,}02485\ldots", size=32, color=GREY_B),
        ).arrange(DOWN, buff=0.4).move_to(RIGHT * 3.4 + UP * 0.2)
        self.play(Write(eqs[0]))
        self.say("Вблизи точки график почти совпадает с касательной. Значит, "
                 "f(a + h) ≈ f(a) + f'(a)·h — чем меньше h, тем точнее.")
        self.play(Write(eqs[1]))
        self.play(Write(eqs[2]))
        self.play(Write(eqs[3]))
        self.say("Для √x при a = 4: производная 1/4, h = 0,1. Приближение 2,025 "
                 "ошибается меньше чем на 0,0002.")
        self.say("На этой идее — локальной линейности — держатся метод Ньютона, "
                 "физические модели, машинное обучение (градиентный спуск) и многое другое.")

    # ═══════════════════════ 15. Итог ════════════════════════════════════
    def ch15_summary(self):
        self.chapter(15, "Итог", "вся история на одном экране")
        steps = ["средняя скорость Δs/Δt", "наклон секущей", "h → 0 даёт 0/0",
                 "предел: поведение рядом", "строго: ε–δ", "f'(x) = lim …",
                 "нет производной: угол, разрыв", "правила из определения", "применения"]
        boxes = VGroup()
        for s in steps:
            t = T(s, 24)
            boxes.add(VGroup(SurroundingRectangle(t, buff=0.15, corner_radius=0.1, color=C_F, stroke_width=2), t))
        boxes.arrange_in_grid(rows=3, cols=3, buff=(0.6, 0.55)).scale_to_fit_width(12.6).to_edge(UP, buff=0.5)
        arrows = VGroup(*[Arrow(a.get_right(), b.get_left(), buff=0.05, stroke_width=3, color=GREY_B,
                                max_tip_length_to_length_ratio=0.3)
                          for a, b in zip(boxes[:-1], boxes[1:]) if abs(a.get_y() - b.get_y()) < 0.1])
        self.play(LaggedStart(*[FadeIn(b, shift=0.2 * UP) for b in boxes], lag_ratio=0.3), run_time=4)
        self.play(LaggedStart(*[GrowArrow(a) for a in arrows], lag_ratio=0.2))
        final = M("f'(x)", "=", r"\lim_{h\to 0}", r"\frac{f(x+h)-f(x)}{h}", size=60)
        final[0].set_color(C_SEC)
        final[2].set_color(C_LIM)
        final.next_to(boxes, DOWN, buff=0.6)
        self.play(Write(final), run_time=2)
        self.play(Circumscribe(final, color=C_SEC, run_time=2))
        self.say("Производная — это не деление на ноль, а предел наклонов секущих. "
                 "Предел позволяет говорить о «мгновенном», не деля 0 на 0.")
        self.say("Из одного определения выросли все правила, а из правил — физика, "
                 "оптимизация и приближённые вычисления.")
        self.unsay()
        self.clear_all()
        thanks = T("Спасибо за внимание!", 56)
        sub = T("Следующий шаг — интеграл: обратная сторона производной", 30, GREY_A).next_to(thanks, DOWN, buff=0.4)
        self.play(Write(thanks))
        self.play(FadeIn(sub, shift=UP * 0.2))
        self.wait(3)
        self.play(FadeOut(thanks), FadeOut(sub))


CHAPTERS = [
    "ch00_intro", "ch01_average_speed", "ch02_secant", "ch03_zero_over_zero",
    "ch04_limit_intuition", "ch05_epsilon_delta", "ch06_derivative", "ch07_no_derivative",
    "ch08_sum_rule", "ch09_power_rule", "ch10_product_rule", "ch11_chain_rule",
    "ch12_exponential", "ch13_logarithm", "ch14_applications", "ch15_summary",
]


class FullVideo(Story):
    """Весь фильм одним роликом."""

    def construct(self):
        for name in CHAPTERS:
            getattr(self, name)()


def _chapter_scene(name, method):
    return type(name, (Story,), {"construct": lambda self: getattr(self, method)()})


Ch00_Intro = _chapter_scene("Ch00_Intro", "ch00_intro")
Ch01_AverageSpeed = _chapter_scene("Ch01_AverageSpeed", "ch01_average_speed")
Ch02_Secant = _chapter_scene("Ch02_Secant", "ch02_secant")
Ch03_ZeroOverZero = _chapter_scene("Ch03_ZeroOverZero", "ch03_zero_over_zero")
Ch04_LimitIntuition = _chapter_scene("Ch04_LimitIntuition", "ch04_limit_intuition")
Ch05_EpsilonDelta = _chapter_scene("Ch05_EpsilonDelta", "ch05_epsilon_delta")
Ch06_Derivative = _chapter_scene("Ch06_Derivative", "ch06_derivative")
Ch07_NoDerivative = _chapter_scene("Ch07_NoDerivative", "ch07_no_derivative")
Ch08_SumRule = _chapter_scene("Ch08_SumRule", "ch08_sum_rule")
Ch09_PowerRule = _chapter_scene("Ch09_PowerRule", "ch09_power_rule")
Ch10_ProductRule = _chapter_scene("Ch10_ProductRule", "ch10_product_rule")
Ch11_ChainRule = _chapter_scene("Ch11_ChainRule", "ch11_chain_rule")
Ch12_Exponential = _chapter_scene("Ch12_Exponential", "ch12_exponential")
Ch13_Logarithm = _chapter_scene("Ch13_Logarithm", "ch13_logarithm")
Ch14_Applications = _chapter_scene("Ch14_Applications", "ch14_applications")
Ch15_Summary = _chapter_scene("Ch15_Summary", "ch15_summary")
