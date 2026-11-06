import { statSync } from "node:fs";
import { isAbsolute, join } from "node:path";
import { execSync } from "node:child_process";

const PATHISH = /[/\\]|\.[A-Za-z0-9]{1,8}$/;

// Tracked and untracked files of the project, for suffix resolution of subfolder-relative spans.
export function createFileIndex(cwd) {
    let cache;
    return () => {
        if (!cache) {
            try {
                cache = execSync("git ls-files -z --cached --others --exclude-standard",
                    { cwd, maxBuffer: 64 * 1024 * 1024, stdio: ["pipe", "pipe", "pipe"] }).toString("utf8").split("\0").filter(Boolean);
            }
            catch { cache = []; }
        }
        return cache;
    };
}

// Href for a code span that names a real file; undefined for everything else.
function hrefFor(span, cwd, getIndex) {
    if (span.length === 0 || span.length > 260 || /[\s`]/.test(span)) return undefined;
    if (!PATHISH.test(span) || /^[a-z]+:\/\//i.test(span)) return undefined;
    const absolute = isAbsolute(span) || /^[A-Za-z]:[\\/]/.test(span);
    if (absolute) {
        try {
            if (!statSync(span).isFile()) return undefined;
        }
        catch { return undefined; }
        return "file:///" + encodeURI(span.split("\\").join("/").replace(/^\/+/, ""));
    }
    const slashed = span.split("\\").join("/");
    try {
        if (statSync(join(cwd, slashed)).isFile()) return encodeURI(slashed);
    }
    catch { /* fall through to the project index */ }
    if (!getIndex || !slashed.includes("/")) return undefined;
    // Models often write paths relative to a subfolder; accept only a unique suffix match.
    const matches = getIndex().filter(file => file === slashed || file.endsWith("/" + slashed));
    return matches.length === 1 ? encodeURI(matches[0]) : undefined;
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

function transform(text, end, cwd, fenced0, getIndex) {
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
                const href = free ? hrefFor(span, cwd, getIndex) : undefined;
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

export function linkifyText(text, cwd, getIndex) {
    return transform(text, text.length, cwd, false, getIndex);
}

// Streaming variant: holds the tail where a code span or fence line may still grow.
export function createFileLinker(cwd, getIndex) {
    let tail = "", fenced = false;
    return {
        push(chunk) {
            const buf = tail + chunk;
            const start = fenced;
            const cut = scan(buf, start);
            fenced = cut.fenced;
            tail = buf.slice(cut.end);
            return transform(buf, cut.end, cwd, start, getIndex);
        },
        flush() {
            const held = tail;
            tail = "";
            return held;
        },
    };
}
