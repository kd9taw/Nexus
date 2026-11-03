# Nexus 简体中文（Chinese, Simplified）本地化套件

Nexus 是业余无线电的“一个窗口搞定全部”程序：CAT 控制电台，收发 FT8、CW、SSB、RTTY、PSK、SSTV 与 APRS，带硬件日志簿、通联地图、spot 列表和卫星跟踪。

程序目前支持英语、德语、西班牙语、法语、日语和俄语。加上简体中文，中文操作员就能用母语上机。

**状态：已完成。** `nexus-zh-translation.csv` 共 **10,876 行，全部填写（100%）**，对应 **10,195 条**界面文案 + 5 条启动页文字。其中 283 行是维护者为新增文案补的译文（`notes` 列写着 “Filled by the maintainer, pending translator review.”），等待译者复核。

---

## 需要做什么

用文本编辑器或表格软件打开 **`nexus-zh-translation.csv`**，填 **`chinese`** 列。

其余各列**不可修改**。程序按 `key` 列匹配行，改 key 或删行就等于丢了一条文案。保存时必须是 **UTF-8** 编码。

| 列 | 说明 |
|---|---|
| `priority` | 优先级，1（最核心界面）到 6。 |
| `key` | 文案的内部标识。**不要改。** |
| `english` | 英文原文。**不要改。** |
| `chinese` | **你的译文。** |
| `do_not_translate` | 必须原样保留在译文里的术语，用 `\|` 分隔。 |
| `notes` | 开发者的上下文提示。 |

空单元格在运行时自动回退成英文原文——不会崩、不会空，只是那一句显示英文。

---

## 主要技术规则（会弄坏程序的那几条）

1. **占位符 `{{双花括号}}`**
   - 原文：`Imported {{count}} QSOs`
   - 译文：`已导入 {{count}} 个 QSO`
   - **禁止**：改括号里的名字（如 `{{数量}}`），或写成单括号 `{count}`。

2. **格式标记 `<b>…</b>`**
   - 原文：`<b>{{achievement}}</b> — turn on <b>{{feature}}</b>?`
   - 译文：`<b>{{achievement}}</b> —— 是否开启 <b>{{feature}}</b>？`
   - 标记必须成对闭合，并且**数量与英文一致**。多写一个不会变成粗体，会在屏幕上原样印出 `<b>`。英文用**全大写**表示强调的地方（如 `RECEIVER`），中文直接写正文即可，不要自己加 `<b>`。

3. **技术数字保留小数点（不是逗号）**
   - `14.074`、`2.4 kHz`、`45.45 baud`。
   - 中文行文里用逗号，但频率与技术参数**永远是点**。频率里出现逗号会让电台设到错误的频点。

4. **CW 宏：单花括号、全大写**
   - `{NAME}`、`{MYCALL}`、`{RST}`、`{MYSTATE}`。原样照抄。

5. **业余无线电术语**
   - `QSO`、`CW`、`RTTY`、`FT8`、`ADIF`、`Cabrillo`、`LoTW`、`QRZ`、`POTA`、`DX`、`QSY`、`CAT` 等保留英文，与空口上的说法一致。哪些词保留、哪些词译成中文，见 [`GLOSSARY.md`](GLOSSARY.md)。

---

## 复数（`::one` / `::few` / `::many` / `::other`）

英文带复数的每一条 key，在 CSV 里有四行：

```csv
logbook.import.imported::one,Imported {{count}} QSO        → 已导入 {{count}} 个 QSO
logbook.import.imported::few,Imported {{count}} QSOs       → 已导入 {{count}} 个 QSO
logbook.import.imported::many,Imported {{count}} QSOs      → 已导入 {{count}} 个 QSO
logbook.import.imported::other,Imported {{count}} QSOs     → 已导入 {{count}} 个 QSO
```

**中文四行写同一句**，一律带 `{{count}}`。`Intl.PluralRules('zh')` 对任何数字都只选 `other`，所以 `zh.ts` 里每条复数文案只是**一个字符串**（`ja.ts` 同一做法），转换器取 `other` 那一行。另外三行不是废行：后期维护者要改这条文案时，四行一起改才不会前后打架。

四行都必须填满。只填一半，`csv-to-catalog.py` 会拒绝生成目录并报错——空着的那一行意味着没人看过的句子。

---

## 优先级

| 级别 | 行数 | 内容 |
|---:|---:|---|
| **1** | **1,124** | 主框架：顶栏、连接状态、错误提示、频率/波段控制、QSO 表单、台站设置。 |
| 2 | 366 | 首次启动：向导、入门指引、启动页。 |
| 3 | 2,092 | 操作面：FT8/FT4 界面、日志簿、spot 列表、Needed 面板、瀑布图。 |
| 4 | 574 | 设置：音频、电台、连接、通知、备份。 |
| 5 | 4,167 | CW、话音、RTTY、PSK、SSTV、APRS、JS8、卫星、奖项、地图。 |
| 6 | 2,553 | 进阶：电台深度控制、旋转器、远程访问。 |
| | **10,876** | 另含 5 行启动页文字（`splashscreen.` 前缀）。 |

---

## 自动检查

改完译文，先重新生成目录（见下一节），再在 `ui/` 里跑目录测试：

```bash
python3 translations/zh/csv-to-catalog.py translations/zh/nexus-zh-translation.csv \
    -o ui/src/i18n/zh.ts --splash-out translations/zh/splash-zh.json
cd ui && npx vitest run src/i18n/placeholders.test.ts src/i18n/splash.test.ts src/i18n/locale-switch.test.ts src/features/macroSets.test.ts
```

转换器 `csv-to-catalog.py` 会拒绝生成并说明原因：列名不对、文件不是 UTF-8、同一个 key 出现两次、复数四行只填了一部分或缺 `other`、启动页文字只填了一部分。英文已改动或已删除的行会被跳过（那句显示当前英文），输出里会报出条数。

`placeholders.test.ts` 逐条查：占位符 `{{...}}` 和格式标记与英文一致、技术数字没有冒出逗号、没有写成文字的 `\uXXXX`、复数没有被拼成两遍，以及英文的每个 key 在 `zh.ts` 里都有译文。`splash.test.ts` 查启动页文字齐全，`macroSets.test.ts` 查宏按钮的名字不会是“停止”这类词。

这些检查都**不评价**中文好不好——那是人的事，程序看不出来。也没有程序逐行核对 `do_not_translate` 列，那一列请人工对照。

---

## 生成 TypeScript 目录（`zh.ts`）

译文定稿后，生成程序真正 import 的目录：

```bash
python3 csv-to-catalog.py nexus-zh-translation.csv -o ../../ui/src/i18n/zh.ts --splash-out splash-zh.json
```

`zh.ts` 是**生成物，不要手改**；要改就改 CSV 再跑一次。

启动页的 5 条文字不进 `zh.ts`：`--splash-out` 把它们写成 JSON，粘到 `ui/public/splashscreen.html` 的 `splash-strings` 块里 `"zh"` 之下。

---

## 已经上线的语言是什么样

`zh.ts` 以 `zh` 注册（**不是** `zh-CN`：上游两处守卫用 `/installCatalog\('([a-z-]+)'/` 抓这一行，大写的 `C` 会让整套译文从守卫里静默掉出去）。`initLocale()` 先匹配语言再匹配地区，所以 `zh` 这套同时服务 `zh-CN`、`zh-Hans`、`zh-SG`。

下面七处与本目录一起构成完整改动：

1. `ui/src/i18n/zh.ts` —— 本目录生成
2. `ui/src/main.tsx` —— `import { ZH }`，并在 `initLocale()` 之前 `installCatalog('zh', ZH)`
3. `ui/src/lan/main.tsx` —— “本网络上的台站”是**第二个 Vite 入口**，同样逐条 `installCatalog`。上游 `docs/i18n.md` 的加语言清单没有写它，照文档做的话这个窗口对中文用户仍是英文
4. `ui/src/i18n/useLocale.ts` —— `LOCALE_NATIVE_NAME` 加 `zh: '简体中文'`
5. `ui/src/i18n/placeholders.test.ts` —— `OTHER` 列表加 `['zh', ZH]`，否则三道路由检查看不到中文
6. `ui/public/splashscreen.html` —— `splash-strings` 加 `"zh"` 块
7. `docs/i18n.md` —— 语言清单，以及中文/日文为何不需要复数对象

`ui/src/tv/main.tsx`（电视大屏）对任何语言都不装目录，故未改动。

---

## 致谢与许可

英文原文、术语约定与校验脚本的形式来自上游 `translations/ru`（loloka）与 `translations/pt-BR` 两套；中文译文由 **BG5EQA**（业余无线电呼号，GitHub [@kmno4891](https://github.com/kmno4891)）整理，借助 AI 助手逐条校对。译文全部经人工复核，术语取舍见 [`GLOSSARY.md`](GLOSSARY.md)。

译文与套件脚本随 Nexus 一同按 **GPL-3.0-only** 发布。
