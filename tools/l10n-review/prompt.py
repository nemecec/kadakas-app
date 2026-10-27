"""Prompt builder and answer parser for translation suggestions."""
import json
import os
import re
import shutil
import subprocess
import tempfile

LANG_NAMES = {"et": "Estonian", "de": "German", "fi": "Finnish", "sv": "Swedish", "lv": "Latvian",
              "lt": "Lithuanian", "fr": "French", "es": "Spanish", "ru": "Russian"}

SEED_GLOSSARY = [
    {"en": "crash report", "tr": "kokkujooksmise aruanne",
     "note": "Owner choice. Older texts say \"jooksutuse aruanne\", which sounds wrong."},
    {"en": "recovery copy", "tr": "taastekoopia", "note": "Used in the autosave texts."},
    {"en": "layer", "tr": "kiht", "note": "Drawing layer. Not a sheet."},
    {"en": "sheet", "tr": "leht", "note": "Layout sheet."},
    {"en": "plant schedule", "tr": "taimede nimekiri", "note": "Table of plants."},
]


def language_name(target):
    return LANG_NAMES.get(target.split("-")[0], target)


MODELS = ["sonnet", "opus", "haiku"]


def build_prompt(target, en, cur, highlights, comment, neighbours, glossary, avoid=None):
    """en and cur are dicts with raw, comment, variables, selector."""
    lang = language_name(target)
    L = []
    L.append("You are a native %s speaker and a careful software translator." % lang)
    L.append("Improve one user-interface text of Kadakas, a landscape design program.")
    L.append("The current %s text was made by a machine and some words sound wrong." % lang)
    L.append("Write natural, plain %s for software UI, as used in the Microsoft, Apple and Mozilla %s interfaces." % (lang, lang))
    L.append("Keep the meaning of the English. Prefer short common words. Be consistent with the fixed terms.")
    L.append("")
    L.append("English source:")
    L.append(en["raw"])
    if en.get("comment"):
        L.append("")
        L.append("Context note from the developers: " + en["comment"].replace("\n", " "))
    L.append("")
    L.append("Current %s text:" % lang)
    L.append(cur["raw"] if cur else "(missing)")
    if highlights:
        L.append("")
        L.append("The owner marked these words as weird: " + ", ".join('"%s"' % h["text"] for h in highlights))
    if comment:
        L.append("")
        L.append("The owner's comment: " + comment)
    if glossary:
        L.append("")
        L.append("Fixed terms (always use these):")
        for g in glossary:
            L.append("- %s -> %s%s" % (g["en"], g["tr"], (" (" + g["note"] + ")") if g.get("note") else ""))
    if neighbours:
        L.append("")
        L.append("Neighbouring texts of the same file, for context (key: English | %s):" % lang)
        for n in neighbours[:6]:
            L.append("- %s: %s | %s" % (n["key"], n["en"].replace("\n", " "), n["tr"].replace("\n", " ")))
    if en.get("variables"):
        L.append("")
        L.append("Keep every variable exactly as written: " + ", ".join("{ $%s }" % v for v in en["variables"]) + ".")
    if en.get("selector"):
        L.append("The text is a Fluent selector. Keep the selector line, the variant keys, the [ ] marks, the * mark and the braces exactly. Change only the words after each variant key. Keep the line breaks and the indentation of the current text.")
    if avoid:
        L.append("")
        L.append("Do not repeat these rejected alternatives, give different ones:")
        for a in avoid:
            L.append("- " + a.replace("\n", " / "))
    L.append("")
    L.append("Reply with strict JSON only, no text outside it, no code fence:")
    L.append('{"alternatives":[{"text":"...","why":"short reason in English","keeps_variables":true}]}')
    L.append("Give exactly 3 alternatives. No leading or trailing spaces in text. Use \\n for line breaks only where the source has them.")
    return "\n".join(L)


def parse_alternatives(output, limit=3):
    """Find the first JSON object in the answer. Returns a list of {text, why}. Raises ValueError."""
    text = output.strip()
    text = re.sub(r"```[a-zA-Z]*", "", text)
    dec = json.JSONDecoder()
    obj = None
    for m in re.finditer(r"\{", text):
        try:
            cand, _ = dec.raw_decode(text[m.start():])
        except ValueError:
            continue
        if isinstance(cand, dict) and isinstance(cand.get("alternatives"), list):
            obj = cand
            break
    if obj is None:
        raise ValueError("No JSON object with \"alternatives\" in the answer.")
    out = []
    for a in obj["alternatives"]:
        if isinstance(a, str):
            a = {"text": a}
        if not isinstance(a, dict) or not isinstance(a.get("text"), str) or not a["text"].strip():
            continue
        out.append({"text": a["text"], "why": str(a.get("why", "")),
                    "claims_variables": a.get("keeps_variables")})
    if not out:
        raise ValueError("The JSON has no usable alternatives.")
    return out[:limit]


class AskError(RuntimeError):
    def __init__(self, message, code=None, stderr=""):
        super().__init__(message)
        self.code = code
        self.stderr = stderr


def ask_claude(prompt, binary="claude", model="sonnet", timeout=120, cancel=None):
    """Run `claude -p` with the prompt on stdin. cwd is a temp dir, never the repo."""
    if model not in MODELS:
        raise AskError("Unknown model: %s" % model)
    exe = shutil.which(binary)
    if not exe:
        raise AskError("The Claude Code CLI (%s) was not found on this machine. Use Copy prompt instead." % binary)
    with tempfile.TemporaryDirectory(prefix="l10n-review-") as cwd:
        try:
            proc = subprocess.Popen([exe, "-p", "--model", model], stdin=subprocess.PIPE,
                                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, cwd=cwd)
        except OSError as e:
            raise AskError("Could not run the Claude Code CLI: %s" % e)
        if cancel is not None:
            cancel.append(proc)
        try:
            out, err = proc.communicate(prompt, timeout=timeout)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.communicate()
            raise AskError("Claude did not answer within %d seconds." % timeout, code="timeout")
        finally:
            if cancel is not None and proc in cancel:
                cancel.remove(proc)
    if proc.returncode != 0:
        tail = "\n".join((err or out or "").strip().split("\n")[-8:])
        raise AskError("Claude failed (exit code %d). It may be missing, not logged in, or out of usage." % proc.returncode,
                       code=proc.returncode, stderr=tail)
    try:
        return parse_alternatives(out)
    except ValueError as e:
        raise AskError("Claude answered, but not in the expected JSON: %s" % e, code=0, stderr=out.strip()[-500:])
