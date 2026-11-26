import { fetch, Body } from '@tauri-apps/api/http';

export async function translate(text, from, to, options = {}) {
    const { config } = options;

    const serviceType = config['type'];
    if (serviceType === 'free') {
        return translate_by_free(text, from, to);
    } else if (serviceType === 'api') {
        return translate_by_key(text, from, to, config.authKey);
    } else if (serviceType === 'deeplx') {
        return translate_by_deeplx(text, from, to, config.customUrl);
    } else {
        return translate_by_free(text, from, to);
    }
}

// DeepL 免费通道。
// 2026 年起旧的 www2.deepl.com/jsonrpc（LMT_handle_texts）会返回
// "You're using an older version of DeepL" 占位文本，不再给译文。
// 改用官方 Chrome 扩展 / iOS app 走的 oneshot 免鉴权端点。
// 参考：https://github.com/OwO-Network/DeepLX（v1.2.0 起同样迁移到这里）
async function translate_by_free(text, from, to) {
    const url = 'https://oneshot-free.www.deepl.com/v1/translate';

    // 该端点匿名调用硬上限 1500 字符，超了直接 400
    if (text.length > 1500) {
        throw `Text too long: ${text.length} characters (free channel limit is 1500)`;
    }

    const body = {
        text: [text],
        target_lang: normalize_target_lang(to),
    };
    if (from !== 'auto' && from !== '') {
        body['source_lang'] = normalize_source_lang(from);
    }

    let res = await fetch(url, {
        method: 'POST',
        body: Body.json(body),
        headers: { 'Content-Type': 'application/json' },
    });

    if (res.ok) {
        let result = res.data;
        if (result && result.translations && result.translations[0]) {
            return result.translations[0].text.trim();
        } else {
            throw JSON.stringify(result);
        }
    } else {
        throw `Http Request Error\nHttp Status: ${res.status}\n${JSON.stringify(res.data)}`;
    }
}

// oneshot 端点只认小写语言码，且对大小写错误不报错——会直接把原文返回。
// 特例：中文必须写 zh-Hans / zh-Hant，葡语必须写 pt-PT / pt-BR。
function normalize_target_lang(to) {
    const upper = (to || '').toUpperCase();
    if (upper === 'ZH' || upper === 'ZH-HANS') {
        return 'zh-Hans';
    }
    if (upper === 'ZH-HANT') {
        return 'zh-Hant';
    }
    if (upper === 'PT-PT') {
        return 'pt-PT';
    }
    if (upper === 'PT-BR') {
        return 'pt-BR';
    }
    return upper.toLowerCase();
}

function normalize_source_lang(from) {
    return normalize_target_lang(from);
}
async function translate_by_deeplx(text, from, to, url) {
    let res = await fetch(url, {
        method: 'POST',
        body: Body.json({
            source_lang: from,
            target_lang: to,
            text: text,
        }),
    });

    if (res.ok) {
        const result = res.data;
        if (result['data']) {
            return result['data'];
        } else {
            throw JSON.stringify(result);
        }
    } else {
        throw `Http Request Error\nHttp Status: ${res.status}\n${JSON.stringify(res.data)}`;
    }
}

async function translate_by_key(text, from, to, key) {
    const headers = {
        'Content-Type': 'application/json',
        Authorization: `DeepL-Auth-Key ${key}`,
    };
    let body = {
        text: [text],
        target_lang: to,
    };
    if (from !== 'auto') {
        body['source_lang'] = from;
    }
    let url;
    if (key.endsWith(':fx')) {
        url = 'https://api-free.deepl.com/v2/translate';
    } else if (key.endsWith(':dp')) {
        url = 'https://api.deepl-pro.com/v2/translate';
    } else {
        url = 'https://api.deepl.com/v2/translate';
    }
    let res = await fetch(url, {
        method: 'POST',
        body: Body.json(body),
        headers: headers,
    });

    if (res.ok) {
        const result = res.data;
        if ((result.translations, result.translations[0])) {
            return result.translations[0].text.trim();
        } else {
            throw JSON.stringify(result);
        }
    } else {
        if (res.data.error) {
            throw `Status Code: ${res.status}\n${res.data.error.message}`;
        } else {
            throw `Http Request Error\nHttp Status: ${res.status}\n${JSON.stringify(res.data)}`;
        }
    }
}

export * from './Config';
export * from './info';
