import { statSync } from "node:fs";
import { isAbsolute, join } from "node:path";

const PATHISH = /[/\\]|\.[A-Za-z0-9]{1,8}$/;

// Href for a code span that names a real file; undefined for everything else.
function hrefFor(span, cwd) {
    if (span.length === 0 || span.length > 260 || /[\s`]/.test(span)) return undefined;
    if (!PATHISH.test(span) || /^[a-z]+:\/\//i.test(span)) return undefined;
    const absolute = isAbsolute(span) || /^[A-Za-z]:[\\/]/.test(span);
    try {
        if (!statSync(absolute ? span : join(cwd, span)).isFile()) return undefined;
    }
    catch { return undefined; }
    const slashed = span.split("\\").join("/");
    return absolute ? "file:///" + encodeURI(slashed.replace(/^\/+/, "")) : encodeURI(slashed);
}

const fenceLine = line => line.trimStart().startsWith("```");

function scan(text, fenced0) {
    let fenced = fenced0, inSpan = false, spanStart = 0, lineStart = 0;
    for (let i = 0; i < text.length; i++) {
        const ch = text[i];
        if (ch === "\n") {
            inSpan = false;
            if (fenceLine(text.slice(lineStart, i))) fenced = !fenced;
            lineStart = i + 1;
        }
        else if (!fenced && ch === "`") {
            if (inSpan) inSpan = false;
            else { inSpan = true; spanStart = i; }
        }
    }
    if (inSpan) return { end: spanStart, fenced };
    if (/^\s*`+$/.test(text.slice(lineStart))) return { end: lineStart, fenced };
    return { end: text.length, fenced };
}

function transform(text, end, cwd, fenced0) {
    let out = "", fenced = fenced0, inSpan = false, spanBuf = "", lineStart = 0;
    for (let i = 0; i < end; i++) {
        const ch = text[i];
        if (ch === "\n") {
            if (inSpan) { out += "`" + spanBuf; inSpan = false; }
            if (fenceLine(text.slice(lineStart, i))) fenced = !fenced;
            out += ch;
            lineStart = i + 1;
        }
        else if (!fenced && ch === "`") {
            if (inSpan) {
                const span = spanBuf;
                const free = text[i - span.length - 1] !== "[" && !(text[i + 1] === "]" && text[i + 2] === "(");
                const href = free ? hrefFor(span, cwd) : undefined;
                out += href ? "[" + span + "](" + href + ")" : "`" + span + "`";
                inSpan = false;
            }
            else { inSpan = true; spanBuf = ""; }
        }
        else if (inSpan) spanBuf += ch;
        else out += ch;
    }
    if (inSpan) out += "`" + spanBuf;
    return out;
}

export function linkifyText(text, cwd) {
    return transform(text, text.length, cwd, false);
}

// Streaming variant: holds the tail where a code span or fence line may still grow.
export function createFileLinker(cwd) {
    let tail = "", fenced = false;
    return {
        push(chunk) {
            const buf = tail + chunk;
            const start = fenced;
            const cut = scan(buf, start);
            fenced = cut.fenced;
            tail = buf.slice(cut.end);
            return transform(buf, cut.end, cwd, start);
        },
        flush() {
            const held = tail;
            tail = "";
            return held;
        },
    };
}
