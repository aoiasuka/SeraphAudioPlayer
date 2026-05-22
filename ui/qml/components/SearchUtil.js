.pragma library

// SearchUtil.js — 全局曲目/专辑/歌手搜索工具
//
// 特性:
//   1. 空格分词: "周杰伦 七里香" → 两个 token, 必须全部命中 (AND)
//   2. 字段前缀: "artist:周杰伦", "title:七", "album:...", 简写 t:/ar:/al:/n:
//   3. 字段权重: 不同字段的命中分数不同 (title > artist > album)
//   4. 前缀加成: 命中字段开头 ×2 (如 "周" 命中 "周杰伦" 比命中 "杨振宁周" 高)
//   5. trim + 全角空格(U+3000)归一为半角
//   6. 大小写不敏感
//
// 主 API:
//   filter(list, query, fields)   — 返回按分数降序、同分稳定的数组
//   match(rec, query, fields)     — 单条 boolean 判定

// 字段权重 (越大越靠前)
var WEIGHT = {
    title: 10,
    artist: 5,
    album: 3,
    name: 8     // 歌手列表的字段名 (artist 聚合)
};

// 前缀命中加成倍率
var PREFIX_BONUS = 2;

// 用户输入的字段前缀 → 实际字段名
var ALIAS = {
    "title":  "title",  "t":  "title",
    "artist": "artist", "ar": "artist", "歌手": "artist",
    "album":  "album",  "al": "album",  "专辑": "album",
    "name":   "name",   "n":  "name"
};

// 把 query 解析为 token 列表
// 返回 [{ field: string|null, text: string-lowercased }]
function parseTokens(query) {
    if (!query) return [];
    var q = String(query).replace(/　/g, " ").trim();
    if (q.length === 0) return [];
    var parts = q.split(/\s+/);
    var tokens = [];
    for (var i = 0; i < parts.length; ++i) {
        var p = parts[i];
        if (!p) continue;
        var colon = p.indexOf(":");
        if (colon > 0 && colon < p.length - 1) {
            var key = p.substring(0, colon).toLowerCase();
            var rest = p.substring(colon + 1);
            if (ALIAS[key] !== undefined && rest.length > 0) {
                tokens.push({ field: ALIAS[key], text: rest.toLowerCase() });
                continue;
            }
        }
        tokens.push({ field: null, text: p.toLowerCase() });
    }
    return tokens;
}

// 对单条记录计算命中分数; 0 表示不匹配
// fields 决定当前视图实际暴露/搜索哪些字段
function scoreRecord(rec, tokens, fields) {
    if (tokens.length === 0) return 1;   // 无 query 时视为命中(占位分)
    if (!rec) return 0;

    // 预提取 lowercase 字段值
    var values = {};
    for (var f = 0; f < fields.length; ++f) {
        var fn = fields[f];
        var raw = rec[fn];
        values[fn] = (raw !== undefined && raw !== null) ? String(raw).toLowerCase() : "";
    }

    var total = 0;
    for (var i = 0; i < tokens.length; ++i) {
        var tok = tokens[i];
        var tokenScore = 0;

        if (tok.field) {
            // 字段前缀模式: 必须在指定字段命中
            if (fields.indexOf(tok.field) < 0) return 0;   // 该视图不支持该字段
            var v = values[tok.field];
            if (!v) return 0;
            var idx = v.indexOf(tok.text);
            if (idx < 0) return 0;
            tokenScore = (WEIGHT[tok.field] || 1) * (idx === 0 ? PREFIX_BONUS : 1);
        } else {
            // 通配 token: 任一字段命中即可, 取最高分
            var best = 0;
            for (var k = 0; k < fields.length; ++k) {
                var fn2 = fields[k];
                var v2 = values[fn2];
                if (!v2) continue;
                var idx2 = v2.indexOf(tok.text);
                if (idx2 < 0) continue;
                var s = (WEIGHT[fn2] || 1) * (idx2 === 0 ? PREFIX_BONUS : 1);
                if (s > best) best = s;
            }
            if (best === 0) return 0;
            tokenScore = best;
        }
        total += tokenScore;
    }
    return total;
}

// 主 API: 过滤 + 按分数降序排序
// list   — QVariantList 或 JS array, 元素需是含字段的对象
// query  — 用户输入字符串
// fields — 字段名数组, 默认 ["title","artist","album"]
// 返回新数组 (空 query 直接返回原 list 引用)
function filter(list, query, fields) {
    if (!list) return [];
    if (!query || String(query).trim().length === 0) return list;
    fields = fields || ["title", "artist", "album"];

    var tokens = parseTokens(query);
    if (tokens.length === 0) return list;

    var hits = [];
    for (var i = 0; i < list.length; ++i) {
        var s = scoreRecord(list[i], tokens, fields);
        if (s > 0) hits.push({ idx: i, score: s, rec: list[i] });
    }
    // 高分在前, 同分稳定 (按原顺序)
    hits.sort(function(a, b) {
        if (b.score !== a.score) return b.score - a.score;
        return a.idx - b.idx;
    });
    var out = [];
    for (var j = 0; j < hits.length; ++j) out.push(hits[j].rec);
    return out;
}

// 单条 boolean 判定 (不需要排序时)
function match(rec, query, fields) {
    if (!query || String(query).trim().length === 0) return true;
    return scoreRecord(rec, parseTokens(query), fields || ["title", "artist", "album"]) > 0;
}
