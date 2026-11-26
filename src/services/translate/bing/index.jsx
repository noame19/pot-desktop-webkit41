import { fetch } from '@tauri-apps/api/http';

// 微软 Edge 浏览器内置翻译接口。
// 2026-08 起微软下线了旧的 token 接口（https://edge.microsoft.com/translate/auth 返回 404），
// 改成同域名下免鉴权的 translatetext 端点：直接 POST 文本数组即可。
// 参考：https://github.com/zhp2333/pot-app-translate-plugin-edge-translate
const UA = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Edg/131.0.0.0';

export async function translate(text, from, to) {
    // 该接口不接受 from=auto，留空即为自动识别
    const source = from === 'auto' ? '' : from;

    const url =
        'https://edge.microsoft.com/translate/translatetext' +
        `?from=${encodeURIComponent(source)}&to=${encodeURIComponent(to)}&isEnterpriseClient=false`;

    const res = await fetch(url, {
        method: 'POST',
        headers: {
            'content-type': 'application/json',
            // 不带浏览器 UA 会返回 400 "Client Browser Version not supported"
            'User-Agent': UA,
        },
        // 该端点接受字符串数组，返回结果按位置一一对应
        body: {
            type: 'Json',
            payload: [text],
        },
    });

    if (!res.ok) {
        throw `Http Request Error\nHttp Status: ${res.status}\n${JSON.stringify(res.data)}`;
    }

    const result = res.data;
    const translated = result?.[0]?.translations?.[0]?.text;
    if (!translated) {
        throw JSON.stringify(result);
    }
    return translated.trim();
}

export * from './Config';
export * from './info';
