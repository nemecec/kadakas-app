"""A small Fluent evaluator for previews. Standard library only.

It renders a message the way the user sees it, with sample values for the variables.
It reads placeables, selectors (plural categories, number and string keys, default variant),
nested selectors, message references, terms with arguments, string literals and NUMBER.
Attributes of the message itself are ignored. Problems show in the text as `{? ...}`.
"""
import itertools
import re

import samples as samplemod

MAX_DEPTH = 10
MAX_COMBOS = 12
ID_RE = re.compile(r"[A-Za-z][A-Za-z0-9_-]*")
NUM_RE = re.compile(r"-?\d+(?:\.\d+)?")
CATEGORY_VALUE = {"zero": "0", "one": "1", "two": "2", "few": "3", "many": "6", "other": "2"}
CATEGORIES = set(CATEGORY_VALUE)


class FluentSyntax(Exception):
    pass


class Num:
    """A number with the count of its fraction digits (1.0 has one, 1 has none)."""

    def __init__(self, value, digits=0):
        self.value, self.digits = float(value), digits

    @staticmethod
    def parse(text):
        t = text.strip().replace(",", ".")
        if not NUM_RE.fullmatch(t):
            return None
        return Num(float(t), len(t.split(".")[1]) if "." in t else 0)

    def __repr__(self):
        return "Num(%r,%d)" % (self.value, self.digits)


def plural_category(num, locale):
    """English and Estonian (and the default): one when the value is 1 without fraction digits."""
    return "one" if num.value == 1 and num.digits == 0 else "other"


def format_num(num, locale):
    sep = "." if locale.lower().startswith("en") else ","
    text = "%.*f" % (num.digits, num.value)
    whole, _, frac = text.partition(".")
    neg = whole.startswith("-")
    whole = whole.lstrip("-")
    min_group = 4 if locale.lower().startswith("en") else 5   # Estonian groups from five digits
    if len(whole) >= min_group:
        gsep = "," if sep == "." else " "
        parts = []
        while whole:
            parts.insert(0, whole[-3:])
            whole = whole[:-3]
        whole = gsep.join(parts)
    return ("-" if neg else "") + whole + (sep + frac if frac else "")


# ---- parser: raw text -> list of elements (str or expression tuple)

class Parser:
    def __init__(self, s):
        self.s, self.pos, self.n = s, 0, len(s)

    def ws(self, newlines=True):
        while self.pos < self.n and (self.s[self.pos] in " \t" or (newlines and self.s[self.pos] in "\r\n")):
            self.pos += 1

    def expect(self, ch):
        self.ws()
        if self.s[self.pos:self.pos + len(ch)] != ch:
            raise FluentSyntax("expected %r at %d" % (ch, self.pos))
        self.pos += len(ch)

    def pattern(self, in_variant=False):
        elems, buf = [], []

        def flush():
            if buf:
                elems.append("".join(buf))
                buf.clear()
        while self.pos < self.n:
            c = self.s[self.pos]
            if c == "{":
                flush()
                self.pos += 1
                elems.append(self.expression())
                self.expect("}")
            elif in_variant and c == "}":
                break
            elif c == "\n":
                j = self.pos + 1
                while j < self.n and self.s[j] in " \t\r\n":
                    j += 1
                if in_variant and (j >= self.n or self.s[j] == "}" or self.s[j] == "["
                                   or (self.s[j] == "*" and self.s[j + 1:j + 2] == "[")):
                    self.pos = j
                    break
                buf.append("\n")
                self.pos = j
            else:
                buf.append(c)
                self.pos += 1
        flush()
        if in_variant:
            if elems and isinstance(elems[0], str):
                elems[0] = elems[0].lstrip()
            if elems and isinstance(elems[-1], str):
                elems[-1] = elems[-1].rstrip()
        return [e for e in elems if e != ""]

    def expression(self):
        self.ws()
        e = self.inline()
        self.ws()
        if self.s[self.pos:self.pos + 2] == "->":
            self.pos += 2
            return ("select", e, self.variants())
        return e

    def variants(self):
        out = []
        while True:
            self.ws()
            if self.pos >= self.n:
                raise FluentSyntax("unterminated selector")
            if self.s[self.pos] == "}":
                break
            default = False
            if self.s[self.pos] == "*":
                default = True
                self.pos += 1
            self.expect("[")
            end = self.s.find("]", self.pos)
            if end < 0:
                raise FluentSyntax("unterminated variant key")
            key = self.s[self.pos:end].strip()
            self.pos = end + 1
            num = Num.parse(key)
            self.ws()
            out.append((num if num is not None else key, default, self.pattern(in_variant=True)))
        if not out:
            raise FluentSyntax("selector without variants")
        return out

    def inline(self):
        self.ws()
        if self.pos >= self.n:
            raise FluentSyntax("unexpected end")
        c = self.s[self.pos]
        if c == "{":
            self.pos += 1
            e = self.expression()
            self.expect("}")
            return e
        if c == "$":
            m = ID_RE.match(self.s, self.pos + 1)
            if not m:
                raise FluentSyntax("bad variable")
            self.pos = m.end()
            return ("var", m.group())
        if c == '"':
            return ("str", self.string())
        m = NUM_RE.match(self.s, self.pos)
        if m and (c.isdigit() or c == "-" and self.s[self.pos + 1:self.pos + 2].isdigit()):
            self.pos = m.end()
            return ("num", Num.parse(m.group()))
        if c == "-":
            m = ID_RE.match(self.s, self.pos + 1)
            if not m:
                raise FluentSyntax("bad term")
            self.pos = m.end()
            attr = self.attr()
            args = self.args() if self.s[self.pos:self.pos + 1] == "(" else ([], {})
            return ("term", m.group(), attr, args)
        m = ID_RE.match(self.s, self.pos)
        if m:
            self.pos = m.end()
            if self.s[self.pos:self.pos + 1] == "(":
                return ("func", m.group(), self.args())
            return ("msg", m.group(), self.attr())
        raise FluentSyntax("unexpected %r at %d" % (c, self.pos))

    def attr(self):
        if self.s[self.pos:self.pos + 1] == ".":
            m = ID_RE.match(self.s, self.pos + 1)
            if m:
                self.pos = m.end()
                return m.group()
        return None

    def string(self):
        self.pos += 1
        out = []
        while self.pos < self.n and self.s[self.pos] != '"':
            c = self.s[self.pos]
            if c == "\\" and self.pos + 1 < self.n:
                nxt = self.s[self.pos + 1]
                if nxt in '"\\{':
                    out.append(nxt)
                    self.pos += 2
                    continue
                if nxt in "uU":
                    ln = 4 if nxt == "u" else 6
                    try:
                        out.append(chr(int(self.s[self.pos + 2:self.pos + 2 + ln], 16)))
                        self.pos += 2 + ln
                        continue
                    except ValueError:
                        raise FluentSyntax("bad escape")
            out.append(c)
            self.pos += 1
        if self.pos >= self.n:
            raise FluentSyntax("unterminated string")
        self.pos += 1
        return "".join(out)

    def args(self):
        self.pos += 1
        pos_args, named = [], {}
        while True:
            self.ws()
            if self.pos >= self.n:
                raise FluentSyntax("unterminated arguments")
            if self.s[self.pos] == ")":
                self.pos += 1
                return pos_args, named
            m = ID_RE.match(self.s, self.pos)
            if m and self.s[m.end():].lstrip(" \t").startswith(":"):
                name = m.group()
                self.pos = self.s.index(":", m.end()) + 1
                named[name] = self.inline()
            else:
                pos_args.append(self.inline())
            self.ws()
            if self.s[self.pos:self.pos + 1] == ",":
                self.pos += 1


def parse(raw):
    p = Parser(raw)
    elems = p.pattern(in_variant=False)
    return elems


# ---- evaluation

class Result:
    def __init__(self):
        self.errors = []

    def err(self, text):
        if text not in self.errors:
            self.errors.append(text)


class Bundle:
    """The messages and terms of one locale: key -> raw value text. Attributes as 'key.attr'."""

    def __init__(self, locale, files):
        self.locale = locale
        self.messages, self.terms, self.attrs = {}, {}, {}
        for f in files.values():
            for m in f.messages:
                self.messages[m.key] = m.raw
                for a in m.attributes:
                    self.attrs[m.key + "." + a["name"]] = a["raw"]
            for t in f.terms:
                key = t.key[1:] if t.key.startswith("-") else t.key
                self.terms[key] = t.raw
                for a in t.attributes:
                    self.attrs["-" + key + "." + a["name"]] = a["raw"]
        self._cache = {}

    def pattern(self, raw):
        if raw not in self._cache:
            self._cache[raw] = parse(raw)
        return self._cache[raw]


class Evaluator:
    def __init__(self, bundle, variables, result):
        self.b, self.vars, self.res = bundle, variables, result
        self.locale = bundle.locale

    def pattern(self, elems, variables, stack, depth):
        out = []
        for e in elems:
            if isinstance(e, str):
                out.append(e)
            else:
                out.append(self.to_text(self.expr(e, variables, stack, depth)))
        return "".join(out)

    def to_text(self, v):
        return format_num(v, self.locale) if isinstance(v, Num) else v

    def expr(self, e, variables, stack, depth):
        kind = e[0]
        if depth > MAX_DEPTH:
            self.res.err("too deep")
            return "{? too deep}"
        if kind == "str":
            return e[1]
        if kind == "num":
            return e[1]
        if kind == "var":
            if e[1] in variables:
                return variables[e[1]]
            self.res.err("missing variable $%s" % e[1])
            return "{? $%s}" % e[1]
        if kind == "select":
            return self.select(e, variables, stack, depth)
        if kind == "msg":
            return self.ref(e[1], e[2], False, {}, variables, stack, depth)
        if kind == "term":
            pos_args, named = e[3]
            args = {k: self.expr(v, variables, stack, depth) for k, v in named.items()}
            return self.ref(e[1], e[2], True, args, variables, stack, depth)
        if kind == "func":
            return self.func(e, variables, stack, depth)
        raise FluentSyntax("unknown expression")

    def ref(self, name, attr, is_term, args, variables, stack, depth):
        label = ("-" if is_term else "") + name + ("." + attr if attr else "")
        key = (is_term, name, attr)
        if key in stack:
            self.res.err("cycle at %s" % label)
            return "{? cycle %s}" % label
        if attr:
            raw = self.b.attrs.get(("-" if is_term else "") + name + "." + attr)
        else:
            raw = (self.b.terms if is_term else self.b.messages).get(name)
        if raw is None:
            self.res.err("unresolved reference %s" % label)
            return "{? %s}" % label
        try:
            elems = self.b.pattern(raw)
        except FluentSyntax as ex:
            self.res.err("syntax error in %s: %s" % (label, ex))
            return "{? %s}" % label
        # a term sees only its own arguments, a message sees the outer variables
        scope = args if is_term else variables
        return self.pattern(elems, scope, stack | {key}, depth + 1)

    def func(self, e, variables, stack, depth):
        name, (pos_args, named) = e[1], e[2]
        if name == "NUMBER":
            if not pos_args:
                self.res.err("NUMBER without argument")
                return "{? NUMBER()}"
            v = self.expr(pos_args[0], variables, stack, depth)
            if isinstance(v, str):
                n = Num.parse(v)
                if n is None:
                    return v
                v = n
            digits = v.digits
            for k, a in named.items():
                av = self.expr(a, variables, stack, depth)
                if k in ("minimumFractionDigits", "maximumFractionDigits") and isinstance(av, Num):
                    digits = max(digits, int(av.value)) if k[0] == "m" and k[1] == "i" else min(digits, int(av.value))
            return Num(v.value, digits)
        if name == "DATETIME":
            return "[date]"
        self.res.err("unknown function %s" % name)
        return "{? %s()}" % name

    def select(self, e, variables, stack, depth):
        sel = self.expr(e[1], variables, stack, depth)
        chosen = None
        for key, default, pat in e[2]:
            if isinstance(key, Num):
                if isinstance(sel, Num) and sel.value == key.value:
                    chosen = pat
                    break
            elif isinstance(sel, Num):
                if key in CATEGORIES and plural_category(sel, self.locale) == key:
                    chosen = pat
                    break
            elif isinstance(sel, str) and sel == key:
                chosen = pat
                break
        if chosen is None:
            for key, default, pat in e[2]:
                if default:
                    chosen = pat
                    break
            else:
                chosen = e[2][0][2]
        return self.pattern(chosen, variables, stack, depth + 1)


def render(raw, bundle, variables):
    """Render raw text. Returns (text, errors)."""
    res = Result()
    try:
        elems = bundle.pattern(raw)
    except FluentSyntax as ex:
        return "{? syntax error: %s}" % ex, ["syntax error: %s" % ex]
    text = Evaluator(bundle, variables, res).pattern(elems, variables, frozenset(), 0)
    return text, res.errors


# ---- sample values and variant combinations

def _selector_var(sel):
    if sel[0] == "var":
        return sel[1]
    if sel[0] == "func" and sel[1] == "NUMBER" and sel[2][0] and sel[2][0][0][0] == "var":
        return sel[2][0][0][1]
    return None


def collect_selectors(elems, bundle, found=None, seen=None):
    """{variable name: ordered list of variant keys} for the selectors of a pattern and its references."""
    found = {} if found is None else found
    seen = set() if seen is None else seen
    for e in elems:
        if isinstance(e, str):
            continue
        _walk(e, bundle, found, seen)
    return found


def _walk(e, bundle, found, seen):
    kind = e[0]
    if kind == "select":
        name = _selector_var(e[1])
        if name:
            keys = found.setdefault(name, [])
            for key, default, pat in e[2]:
                k = key.value if isinstance(key, Num) else key
                if k not in keys:
                    keys.append(k)
        else:
            _walk(e[1], bundle, found, seen)
        for key, default, pat in e[2]:
            for x in pat:
                if not isinstance(x, str):
                    _walk(x, bundle, found, seen)
    elif kind == "msg" and not e[2]:
        if e[1] not in seen and e[1] in bundle.messages:
            seen.add(e[1])
            try:
                collect_selectors(bundle.pattern(bundle.messages[e[1]]), bundle, found, seen)
            except FluentSyntax:
                pass
    elif kind == "func":
        for a in e[2][0]:
            _walk(a, bundle, found, seen)


def collect_vars(elems, bundle, found=None, seen=None):
    """Variable names used by a pattern and by the messages it references (terms have their own scope)."""
    found = [] if found is None else found
    seen = set() if seen is None else seen

    def walk(e):
        kind = e[0]
        if kind == "var":
            if e[1] not in found:
                found.append(e[1])
        elif kind == "select":
            walk(e[1])
            for key, default, pat in e[2]:
                for x in pat:
                    if not isinstance(x, str):
                        walk(x)
        elif kind == "msg":
            if e[1] not in seen and e[1] in bundle.messages:
                seen.add(e[1])
                try:
                    collect_vars(bundle.pattern(bundle.messages[e[1]]), bundle, found, seen)
                except FluentSyntax:
                    pass
        elif kind == "func":
            for a in e[2][0]:
                walk(a)
    for e in elems:
        if not isinstance(e, str):
            walk(e)
    return found


def test_values(keys):
    """Values for one selector variable that reach every key: numbers and plural categories."""
    vals = []
    stringy = any(isinstance(k, str) and k not in CATEGORIES for k in keys)   # a string selector
    for k in keys:
        if isinstance(k, float):
            v = ("%g" % k)
        elif k in CATEGORIES and not stringy:
            v = CATEGORY_VALUE[k]
        else:
            v = k
        if v not in vals:
            vals.append(v)
    nums = sorted((v for v in vals if Num.parse(v) is not None), key=lambda x: Num.parse(x).value)
    return nums + [v for v in vals if Num.parse(v) is None]


def _to_var(text):
    n = Num.parse(text)
    return n if n is not None else text


def previews(raw, bundle, owner_samples=None, extra_vars=()):
    """Preview lines for a raw text: [{label, text, errors}] and the variable names with sample values.

    One line per combination of selector values, at most MAX_COMBOS.
    """
    owner = owner_samples or {}
    try:
        elems = bundle.pattern(raw)
    except FluentSyntax as ex:
        return {"lines": [{"label": "", "text": "{? syntax error: %s}" % ex, "errors": ["syntax error: %s" % ex]}],
                "truncated": False}
    names = list(dict.fromkeys(collect_vars(elems, bundle) + list(extra_vars)))
    selectors = collect_selectors(elems, bundle)
    axes = []
    for name in sorted(selectors):
        if owner.get(name):
            continue   # the owner fixed this value, so it does not vary
        axes.append((name, test_values(selectors[name])))
    base = {n: _to_var(samplemod.sample_for(n, owner)) for n in names}
    combos = list(itertools.product(*[vals for _, vals in axes])) if axes else [()]
    truncated = len(combos) > MAX_COMBOS
    lines = []
    for combo in combos[:MAX_COMBOS]:
        variables = dict(base)
        label = []
        for (name, _), val in zip(axes, combo):
            variables[name] = _to_var(val)
            label.append("%s = %s" % (name, val.replace(".", ",") if not bundle.locale.lower().startswith("en") else val))
        text, errors = render(raw, bundle, variables)
        lines.append({"label": ", ".join(label), "text": text, "errors": errors})
    return {"lines": lines, "truncated": truncated}
