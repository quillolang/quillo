#!/usr/bin/env python3
"""Counts how Quillo functions are written: `fn f() = expr` bodies (and how many fit on one
line), block bodies and how they end (`return`, a bare value, markup, `let`, an `if`), routes and
components by their last statement, and arrows. Reads .qwe files, and the ```quillo blocks of .md
files, under each folder given (FEEDBACK52.md):

    python3 -I feedback/tools/fn-forms.py app ../quillo-website

A heuristic scanner (strings, templates and comments masked; brackets and markup tags counted),
not Quillo's parser: take its numbers as estimates."""
import os, re, sys, json
from collections import Counter

def mask(src):
    out, i, n = [], 0, len(src)
    while i < n:
        c = src[i]
        if c == '/' and src.startswith('//', i):
            j = src.find('\n', i)
            j = n if j < 0 else j
            out.append(' ' * (j - i)); i = j; continue
        if c == '/' and src.startswith('/*', i):
            j = src.find('*/', i + 2)
            j = n if j < 0 else j + 2
            out.append(''.join(ch if ch == '\n' else ' ' for ch in src[i:j])); i = j; continue
        if c in '"`':
            j = i + 1
            while j < n and src[j] != c:
                if src[j] == '\\': j += 1
                elif c == '"' and src[j] == '\n': break
                j += 1
            j = min(j + 1, n)
            out.append(c + ''.join(ch if ch == '\n' else ' ' for ch in src[i + 1:j - 1]) + c); i = j; continue
        out.append(c); i += 1
    return ''.join(out)

PAIR = {'(': ')', '[': ']', '{': '}'}
def match_close(m, i):
    """index of the bracket closing the one at m[i]"""
    d = 0
    for j in range(i, len(m)):
        ch = m[j]
        if ch in '([{': d += 1
        elif ch in ')]}':
            d -= 1
            if d == 0: return j
    return -1

DEF = re.compile(r'(?m)^([ \t]*)(?:(?:export|private|public|server)\s+)*(fn|view|component)\s+([A-Za-z_]\w*)\s*\(')
ROUTE = re.compile(r'(?m)^([ \t]*)route\s+[A-Z]+\s+\S+[^\n{]*\{')
CONT_START = ('|', '.', '&&', '||', '??', '?', ':', 'else', '}', ')', ']', '*', '%', '==', '!=', '+ ', '- ', '=>', '</')
CONT_END = ('+', '=', ',', '(', '[', '{', '|', '&&', '||', '??', '=>', '?', ':')

def body_kind(m, i):
    """after a def's `)`: ('expr', index of '=') or ('block', index of '{')"""
    n = len(m); j = i
    while j < n:
        ch = m[j]
        if ch == '-' and m.startswith('->', j):
            j += 2
            while j < n and m[j] in ' \t': j += 1
            if j < n and m[j] == '{': j = match_close(m, j) + 1
            depth = 0
            while j < n:
                ch = m[j]
                if ch == '<': depth += 1
                elif ch == '>': depth -= 1
                elif ch in '([{' and depth > 0: j = match_close(m, j)
                elif depth == 0 and (ch in '={\n' or m.startswith('with', j) or m.startswith('for ', j) or ch == '!'): break
                j += 1
            continue
        if ch == '[': j = match_close(m, j) + 1; continue
        if ch == '=' and not m.startswith('==', j) and not m.startswith('=>', j): return 'expr', j
        if ch == '{': return 'block', j
        j += 1
    return None, -1

def last_stmt(body):
    lines = body.split('\n')
    depth = 0; tags = 0; start = None; prev_end = ''
    for ln in lines:
        s = ln.strip()
        if s and depth == 0 and tags == 0:
            if not (s.startswith(CONT_START) or prev_end.endswith(CONT_END)):
                start = s
        for t in re.findall(r'<(/?)([A-Za-z][\w.\-]*)[^<>]*?(/?)>', ln) if depth == 0 else []:
            if t[0]: tags -= 1
            elif not t[2]: tags += 1
        tags = max(tags, 0)
        for ch in ln:
            if ch in '([{': depth += 1
            elif ch in ')]}': depth -= 1
        if s: prev_end = s
    if start is None: return 'empty'
    w = re.match(r'[A-Za-z_]\w*', start)
    w = w.group(0) if w else ''
    if w == 'return': return 'return'
    if w == 'reply': return 'reply'
    if w in ('let', 'state'): return 'let'
    if w in ('if', 'match'): return 'if/match'
    if w in ('for', 'while'): return 'loop'
    if start.startswith('<'): return 'markup'
    if re.match(r'[A-Za-z_][\w.]*\s*(=|\+=|-=)(?![=>])', start): return 'assign'
    return 'value'

ARROW = re.compile(r'(?:[(,=|:?]|\breturn)\s*(\([^()]*\)|[a-z_]\w*)\s*(?::\s*[\w<>|?]+\s*)?=>')

def scan(src, c, oneline_examples):
    m = mask(src)
    for d in DEF.finditer(m):
        indent, kw, name = d.group(1), d.group(2), d.group(3)
        p = match_close(m, d.end() - 1)
        if p < 0: continue
        kind, at = body_kind(m, p + 1)
        if kind is None: continue
        if kind == 'expr':
            c[f'{kw} = expr'] += 1
            # one line: nothing indented further follows the def line
            eol = m.find('\n', at); eol = len(m) if eol < 0 else eol
            rest = m[eol + 1:].split('\n')
            nxt = next((l for l in rest if l.strip()), '')
            one = len(nxt) - len(nxt.lstrip()) <= len(indent) and not m[at + 1:eol].strip().endswith(CONT_END)
            if one: c[f'{kw} = expr, one line'] += 1
            c['top-level' if indent == '' else 'nested'] += 0
        else:
            e = match_close(m, at)
            if e < 0: continue
            c[f'{kw} {{}}'] += 1
            c[f'{kw} {{}} ends: {last_stmt(m[at + 1:e])}'] += 1
    for r in ROUTE.finditer(m):
        at = r.end() - 1; e = match_close(m, at)
        if e < 0: continue
        c['route {}'] += 1
        c[f'route ends: {last_stmt(m[at + 1:e])}'] += 1
    for a in ARROW.finditer(m):
        j = a.end()
        while j < len(m) and m[j] in ' \t': j += 1
        c['arrow { }' if j < len(m) and m[j] == '{' else 'arrow expr'] += 1
    c['named arrow (top-level let)'] += len(re.findall(r'(?m)^let\s+\w+\s*=\s*(?:\([^()]*\)|\w+)\s*(?::\s*[\w<>|?]+\s*)?=>', m))
    c['return'] += len(re.findall(r'\breturn\b', m))

FENCE = re.compile(r'```(?:quillo|qwe)\n(.*?)```', re.S)

def main():
    roots = sys.argv[1:]
    for root in roots:
        if os.path.isfile(root):
            c = Counter(); t = open(root, errors='replace').read()
            scan(t if root.endswith('.qwe') else '\n'.join(FENCE.findall(t)), c, None)
            print(json.dumps({'root': root, 'files': 1, **{k: v for k, v in sorted(c.items()) if v}}, indent=1))
            continue
        c = Counter(); files = 0
        for dp, dn, fn in os.walk(root):
            dn[:] = [d for d in dn if d not in ('node_modules', 'target', '.git')]
            for f in fn:
                p = os.path.join(dp, f)
                if f.endswith('.qwe'):
                    files += 1; scan(open(p, errors='replace').read(), c, None)
                elif f.endswith('.md') and '/feedback/reports' not in p:
                    t = open(p, errors='replace').read()
                    blocks = FENCE.findall(t)
                    if blocks:
                        files += 1; scan('\n'.join(blocks), c, None)
        print(json.dumps({'root': root, 'files': files, **{k: v for k, v in sorted(c.items()) if v}}, indent=1))

main()
