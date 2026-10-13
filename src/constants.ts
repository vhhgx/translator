/**
 * 静态兜底数据。
 *
 * 正常运行时这些来自 Rust（`get_meta`），但元数据到达前、或在没有 Tauri IPC
 * 的环境（如用浏览器直接调试前端）下，界面不应该空着——语言下拉至少要可用。
 * 与 `src-tauri/src/prompt.rs` 的 SUPPORTED_LANGUAGES 保持一致。
 */

export const FALLBACK_LANGUAGES: [string, string][] = [
  ['zh', '中文'],
  ['en', '英语'],
  ['fr', '法语'],
  ['pt', '葡萄牙语'],
  ['es', '西班牙语'],
  ['ja', '日语'],
  ['tr', '土耳其语'],
  ['ru', '俄语'],
  ['ar', '阿拉伯语'],
  ['ko', '韩语'],
  ['th', '泰语'],
  ['it', '意大利语'],
  ['de', '德语'],
  ['vi', '越南语'],
  ['ms', '马来语'],
  ['id', '印尼语'],
  ['tl', '菲律宾语'],
  ['hi', '印地语'],
  ['zh-Hant', '繁体中文'],
  ['pl', '波兰语'],
  ['cs', '捷克语'],
  ['nl', '荷兰语'],
  ['km', '高棉语'],
  ['my', '缅甸语'],
  ['fa', '波斯语'],
  ['gu', '古吉拉特语'],
  ['ur', '乌尔都语'],
  ['te', '泰卢固语'],
  ['mr', '马拉地语'],
  ['he', '希伯来语'],
  ['bn', '孟加拉语'],
  ['ta', '泰米尔语'],
  ['uk', '乌克兰语'],
  ['bo', '藏语'],
  ['kk', '哈萨克语'],
  ['mn', '蒙古语'],
  ['ug', '维吾尔语'],
  ['yue', '粤语']
]

export const FALLBACK_STYLES = [
  '日常口语化风格',
  '学术论文严谨风格',
  '科普语气',
  '商业正式语气',
  '新闻报道风格',
  '宣传文案风格',
  '小说风格',
  '法律合同风格',
  '中国古代文言文',
  '技术文档风格',
  '字幕口语风格',
  '儿童易懂风格'
]

export const FALLBACK_FORMAT_TYPES = ['JSON', 'HTML', 'XML', 'YAML', 'Markdown']

export const FALLBACK_MODES: [string, string][] = [
  ['basic', '基础翻译'],
  ['terminology', '术语约束'],
  ['style', '风格控制'],
  ['context', '背景语境'],
  ['delimiter', '分隔符保留'],
  ['structured', '结构化数据']
]
