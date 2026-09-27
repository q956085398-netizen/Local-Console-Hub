#!/usr/bin/env node
/**
 * ui-vision — 零依赖 MCP stdio 服务器
 *
 * 目的：给没有视觉能力的主力模型（如 GLM-5.3）外接一个视觉模型
 *      （默认 GLM-5.3-Flash），让"需要看图"的场景自动走工具调用。
 *
 * 工具：
 *   analyze_design  — 把 UI 设计参考图转写成结构化文字设计规格
 *   compare_ui      — 对比"实现截图 vs 设计参考图"，输出按严重程度排序的差异清单
 *   describe_image  — 通用图像问答（报错截图 / 图标素材 / 任意图片）
 *
 * 环境变量：
 *   UI_VISION_API_KEY   必填（也可用 ZHIPU_API_KEY 或 ZAI_API_KEY 代替）
 *   UI_VISION_MODEL     可选，默认 glm-5.3-flash
 *   UI_VISION_BASE_URL  可选，默认 https://open.bigmodel.cn/api/paas/v4
 *                       （OpenAI 兼容接口；可改为 https://api.z.ai/api/paas/v4
 *                        或本地 vLLM / Ollama 的兼容端点）
 *
 * 运行要求：Node >= 18（内置 fetch），无任何 npm 依赖。
 *
 * 手动冒烟测试（不经 Claude Code）：
 *   UI_VISION_API_KEY=xxx node tools/vision-mcp/server.mjs
 *   然后逐行粘贴：
 *   {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}
 *   {"jsonrpc":"2.0","id":2,"method":"tools/list"}
 *   {"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"describe_image","arguments":{"image_path":"assets/ui/ui-v2-service.png","question":"这张图是什么"}}}
 */
import readline from 'node:readline';
import fs from 'node:fs';
import path from 'node:path';

const MODEL = process.env.UI_VISION_MODEL || 'glm-5.3-flash';
const BASE_URL = (process.env.UI_VISION_BASE_URL || 'https://open.bigmodel.cn/api/paas/v4').replace(/\/+$/, '');
const API_KEY = process.env.UI_VISION_API_KEY || process.env.ZHIPU_API_KEY || process.env.ZAI_API_KEY || '';

const MIME = {
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.webp': 'image/webp',
  '.gif': 'image/gif',
  '.bmp': 'image/bmp',
};

const ANALYZE_PROMPT = `你是资深 UI 工程师。请把这张 UI 设计参考图转写成一份结构化文字设计规格，供没有视觉能力的工程师照着逐像素实现。用 markdown 输出，必须包含以下章节：
1. 整体布局：窗口/页面分区结构（用缩进树描述层级、相对位置与大致比例）
2. 组件清单：逐个列出可见组件（按钮/列表行/标签/图标/输入框/状态点…），说明其位置、文字内容、当前状态
3. 颜色系统：背景/表面/边框/正文/次要文字/主色/状态色，尽量给出估计 hex 值，注明是深色还是浅色主题
4. 字体与排版：字号层级（估计 px）、字重、等宽字体用在哪些位置
5. 间距与形状：关键区域内外边距的相对关系、圆角大小、边框粗细
6. 状态与语义：状态点颜色语义、选中/悬停态的可推断表现
7. 不确定项：无法确认的细节单独列出并标注"推测"
要求：只描述图里真实可见的内容，禁止发明图里不存在的东西。`;

const COMPARE_PROMPT = `你是严格的 UI 走查审查员。图1 是"实现截图"，图2 是"设计参考图"。请逐区域对比两者，输出：
1. 差异清单，按严重程度排序（MAJOR = 布局错误/组件缺失/颜色语义错误；MINOR = 间距/字号/圆角/明暗偏差），每条格式：[MAJOR|MINOR] 位置 — 差异 — 建议修改
2. 实现中多出的或缺失的元素
3. 最后一行单独给出结论，三选一：VERDICT: PASS / VERDICT: MINOR / VERDICT: MAJOR
只报告真实可见的差异，不要为凑数而报告。`;

function loadImage(p) {
  if (typeof p !== 'string' || !p.trim()) throw new Error('缺少图片路径参数');
  const abs = path.resolve(p.trim());
  if (!fs.existsSync(abs)) throw new Error(`图片不存在: ${abs}（相对路径按当前工作目录解析）`);
  const ext = path.extname(abs).toLowerCase();
  const mime = MIME[ext];
  if (!mime) throw new Error(`不支持的图片格式 "${ext || '无扩展名'}"，支持: ${Object.keys(MIME).join(' ')}`);
  const buf = fs.readFileSync(abs);
  if (buf.length > 10 * 1024 * 1024) {
    throw new Error(`图片过大（${(buf.length / 1048576).toFixed(1)}MB），请压缩到 10MB 以内`);
  }
  return { url: `data:${mime};base64,${buf.toString('base64')}` };
}

function extractText(data) {
  const c = data?.choices?.[0]?.message?.content;
  if (typeof c === 'string') return c;
  if (Array.isArray(c)) {
    const t = c.filter((p) => p?.type === 'text').map((p) => p.text).join('\n');
    if (t.trim()) return t;
  }
  throw new Error(`视觉模型返回了无法解析的内容: ${JSON.stringify(data).slice(0, 400)}`);
}

async function callVision(text, images) {
  if (!API_KEY) {
    throw new Error('缺少 API Key：请设置环境变量 UI_VISION_API_KEY（或 ZHIPU_API_KEY / ZAI_API_KEY）后重启会话');
  }
  const content = [{ type: 'text', text }];
  for (const img of images) content.push({ type: 'image_url', image_url: { url: img.url } });

  let res;
  try {
    res = await fetch(`${BASE_URL}/chat/completions`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${API_KEY}` },
      body: JSON.stringify({
        model: MODEL,
        temperature: 0.2,
        max_tokens: 4096,
        messages: [{ role: 'user', content }],
      }),
      signal: AbortSignal.timeout(180000),
    });
  } catch (e) {
    throw new Error(`请求视觉模型失败（${BASE_URL}）: ${e.message}`);
  }

  const raw = await res.text();
  if (!res.ok) {
    throw new Error(
      `视觉模型 API 返回 HTTP ${res.status}: ${raw.slice(0, 300)} — 请检查 API Key、UI_VISION_MODEL（当前: ${MODEL}）与 UI_VISION_BASE_URL（当前: ${BASE_URL}）`
    );
  }
  let data;
  try {
    data = JSON.parse(raw);
  } catch {
    throw new Error(`视觉模型返回非 JSON 响应: ${raw.slice(0, 300)}`);
  }
  if (data?.error) throw new Error(`视觉模型 API 错误: ${JSON.stringify(data.error).slice(0, 300)}`);
  return extractText(data);
}

const TOOLS = [
  {
    name: 'analyze_design',
    description:
      '把 UI 设计参考图转写成结构化文字设计规格（布局树/组件清单/颜色hex/字号/间距/状态语义）。任何 UI 开发开工前必须先用它读取设计图。图片路径如 assets/ui/ui-v2-service.png。',
    inputSchema: {
      type: 'object',
      properties: {
        image_path: { type: 'string', description: '设计参考图路径（相对仓库根或绝对路径）' },
        focus: { type: 'string', description: '可选：本次重点关注的方向' },
      },
      required: ['image_path'],
    },
  },
  {
    name: 'compare_ui',
    description:
      '对比"实现截图 vs 设计参考图"，返回按严重程度排序的差异清单与 VERDICT（PASS/MINOR/MAJOR）。UI 变更完成后用它验收。screenshot_path=实现截图，reference_path=设计参考图。',
    inputSchema: {
      type: 'object',
      properties: {
        screenshot_path: { type: 'string', description: '实现效果截图路径' },
        reference_path: { type: 'string', description: '设计参考图路径，如 assets/ui/ui-v2-service.png' },
        note: { type: 'string', description: '可选：本次对比的关注点' },
      },
      required: ['screenshot_path', 'reference_path'],
    },
  },
  {
    name: 'describe_image',
    description:
      '通用图像问答：查看任意图片（报错截图/图标/素材/照片）并回答问题。凡需要"看图"而你自己无法看图时使用。',
    inputSchema: {
      type: 'object',
      properties: {
        image_path: { type: 'string', description: '图片路径' },
        question: { type: 'string', description: '可选：想问的问题，默认详细描述图片内容' },
      },
      required: ['image_path'],
    },
  },
];

async function dispatch(name, args) {
  if (name === 'analyze_design') {
    const img = loadImage(args.image_path);
    let prompt = ANALYZE_PROMPT;
    if (args.focus) prompt += `\n重点关注：${args.focus}`;
    return callVision(prompt, [img]);
  }
  if (name === 'compare_ui') {
    const shot = loadImage(args.screenshot_path);
    const ref = loadImage(args.reference_path);
    let prompt = COMPARE_PROMPT;
    if (args.note) prompt += `\n本次关注：${args.note}`;
    return callVision(prompt, [shot, ref]);
  }
  if (name === 'describe_image') {
    const img = loadImage(args.image_path);
    const q = typeof args.question === 'string' && args.question.trim() ? args.question.trim() : '请详细描述这张图片的内容。';
    return callVision(q, [img]);
  }
  throw new Error(`未知工具: ${name}`);
}

function send(obj) {
  process.stdout.write(JSON.stringify(obj) + '\n');
}

const rl = readline.createInterface({ input: process.stdin, terminal: false });
rl.on('line', (line) => {
  const s = line.trim();
  if (!s) return;
  let msg;
  try {
    msg = JSON.parse(s);
  } catch {
    return; // 忽略无法解析的行
  }
  if (msg.id === undefined || msg.id === null) return; // notification，无需响应

  const { id, method, params } = msg;
  if (method === 'initialize') {
    send({
      jsonrpc: '2.0',
      id,
      result: {
        protocolVersion: params?.protocolVersion || '2025-06-18',
        capabilities: { tools: {} },
        serverInfo: { name: 'ui-vision', version: '1.0.0' },
      },
    });
  } else if (method === 'tools/list') {
    send({ jsonrpc: '2.0', id, result: { tools: TOOLS } });
  } else if (method === 'tools/call') {
    dispatch(params?.name, params?.arguments || {})
      .then((text) => send({ jsonrpc: '2.0', id, result: { content: [{ type: 'text', text }] } }))
      .catch((err) =>
        send({
          jsonrpc: '2.0',
          id,
          result: { content: [{ type: 'text', text: `ui-vision 工具调用失败: ${err.message}` }], isError: true },
        })
      );
  } else if (method === 'ping') {
    send({ jsonrpc: '2.0', id, result: {} });
  } else {
    send({ jsonrpc: '2.0', id, error: { code: -32601, message: `Method not found: ${method}` } });
  }
});

process.stderr.write(
  `[ui-vision] model=${MODEL} baseUrl=${BASE_URL} apiKey=${API_KEY ? '已配置' : '未配置(调用时会报错)'}\n`
);
