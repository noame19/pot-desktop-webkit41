import { fetch } from '@tauri-apps/api/http';

// Yandex 翻译网页版内部接口。
// 2026 年起旧的 /api/v1/tr.json/translate?srv=android 直接返回 410
// （"Yandex.Translate API v1 is no longer active"），现在必须带一个有效的 SID。
// SID 从 Yandex 自家的网页翻译代理页里抓，抓一次可以用一段时间。
// 参考：https://github.com/FilipePS/Traduzir-paginas-web
const SID_PAGE = 'https://translated.turbopages.org/proxy_u/en-es.en/https/example.com/';
const TRANSLATE_URL = 'https://translate.yandex.net/api/v1/tr.json/translate';

const UA = 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36';

// SID 缓存 20 分钟
const SID_TTL = 20 * 60 * 1000;
let cachedSid = null;
let cachedSidTime = 0;

async function getSid(forceRefresh = false) {
    const now = Date.now();
    if (!forceRefresh && cachedSid && now - cachedSidTime < SID_TTL) {
        return cachedSid;
    }

    const res = await fetch(SID_PAGE, {
        method: 'GET',
        headers: {
            'User-Agent': UA,
        },
        responseType: 2,
    });

    if (!res.ok) {
        throw `Get SID Failed\nHttp Status: ${res.status}`;
    }

    const match = String(res.data).match(/sid:\s*'([0-9a-f.]+)/);
    if (!match || !match[1]) {
        throw `Get SID Failed: sid not found in page`;
    }

    cachedSid = match[1];
    cachedSidTime = now;
    return cachedSid;
}

async function request(text, from, to, sid) {
    // from 为空表示自动识别，此时 lang 只传目标语言
    const lang = from === '' || from === 'auto' ? to : `${from}-${to}`;

    return await fetch(TRANSLATE_URL, {
        method: 'GET',
        headers: {
            'User-Agent': UA,
        },
        query: {
            id: `${sid}-0-0`,
            srv: 'tr-url-widget',
            format: 'text',
            lang: lang,
            text: text,
        },
    });
}

export async function translate(text, from, to) {
    let sid = await getSid();
    let res = await request(text, from, to, sid);

    // SID 过期时返回 403 + "Session is invalid"，换一个新的重试一次
    if (!res.ok && res.status === 403) {
        sid = await getSid(true);
        res = await request(text, from, to, sid);
    }

    if (!res.ok) {
        throw `Http Request Error\nHttp Status: ${res.status}\n${JSON.stringify(res.data)}`;
    }

    const result = res.data;
    if (result && result.text && result.text.length > 0) {
        return result.text[0];
    }
    throw JSON.stringify(result);
}

export * from './Config';
export * from './info';
