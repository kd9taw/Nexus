# Nexus 简体中文术语表（zh）—— 重复出现的词怎么统一

`nexus-zh-translation.csv` 共 **10,515 行**，对应 **9,856 条**界面文案（其中 218 条在英文里带复数形式）加上 5 条启动页文字。约六十个业余无线电术语散落在几百行里；同一个词在不同界面译得不一样，界面就会显得不专业。

下面的数字是这套译文编写时的统计快照（英文侧按整词、按行统计），上游每次新增文案都会让它小幅浮动——看量级就够了，不必逐位对齐。

---

## A. 保留拉丁字母（不译）

国际通联惯例、协议名、文件名、服务名、程序名。中国业余无线电爱好者在电台上、日志里、电台面板上都用这些写法，译成中文反而认不出来。

校验脚本 `verify-zh.py` 通过 `do_not_translate` 列检查它们是否原样保留。

| 术语 | 行数 | 为什么不译 |
|---|---:|---|
| Nexus | 482 | 程序名。 |
| LoTW | 190 | Logbook of The World（ARRL）服务名。 |
| QSO | 125 | 一次通联的国际写法；界面标签与日志字段都用它。 |
| CQ | 128 | 普遍呼叫。 |
| QRZ | 110 | Q 简语，也是呼号数据库网站名。 |
| CW | 108 | 等幅电报（模式名）。 |
| CAT | 107 | Computer Aided Transceiver，电台控制接口。 |
| MHz | 90 | 频率单位，程序内不做本地化。 |
| ADIF | 76 | 日志交换标准格式。 |
| DXCC | 69 | 奖项与地域表。 |
| JS8 | 59 | 模式名（JS8Call 的协议）。 |
| SSTV | 53 | 业余电视。 |
| POTA | 49 | Parks on the Air 激活体系。 |
| FT8 | 47 | 弱信号模式名。 |
| RTTY | 45 | 无线电传。 |
| APRS | 37 | 自动位置报告系统。 |
| CHIRP | 25 | 写频软件名。 |
| Hamlib | 25 | 电台控制库名。 |
| DXpedition | 20 | DX 远征；本套件写作“DX 远征”，`DX` 保留拉丁字母。 |
| SOTA | 17 | Summits on the Air。 |
| IARU / ITU / ARRL / FCC / RSGB | — | 组织名。 |
| WSJT-X / JTDX / MSHV / JS8Call / fldigi / VarAC / FreeDV | — | 程序名。 |
| RST / SNR / DT / WPM / ALC / VFO / AGC / DSP / IMD | — | 技术量与缩写。 |
| QSY / QRM / QRN / QSB / QTH | — | Q 简语在**正文叙述**中保留；作为界面按钮时按下节处理。 |

---

## B. 译为中文（本套件与俄语套件不同之处）

`do_not_translate` 列由俄语译者编制。俄语在电台上保留 `TX`、`RX`、`SWR` 这些拉丁缩写，所以俄语译文照抄即可。**中文不这样做**：界面上那颗按钮写的是“发射”，提示语若还写 `TX`，就相当于指着一个屏幕上并不存在的控件。

因此这套译文把这些词译成中文，并在 `verify-zh.py` 顶部的 `TRANSLATED` 集合里登记，让校验器不再要求它们以拉丁字母出现。

| 英文 | 中文 | 行数 |
|---|---|---:|
| TX | 发射 | 157 |
| Remote | 远程 | 119 |
| RX | 接收 | 96 |
| RF | 射频 | 41 |
| Tune | 调谐 | 30 |
| Digital | 数字 | 27 |
| DXpedition / DXpeditions / DXped | DX 远征 | 20 / 8 / 2 |
| Field Day | 野外日 | 19 |
| AF | 音频 | 9 |
| SWR | 驻波比 | 7 |
| Maidenhead | 梅登海德定位格 | 7 |
| ATU | 天调 | 5 |
| PA | 功放 | 4 |
| Hound | 猎手 | 4 |
| QRM | 干扰 | 4 |
| Baudot | 博多 | 3 |
| AUTOREPLY | 自动回复 | — |
| Stop TX / TX On | 停止发射 / 开启 TX | — |

`DX` 本身保留：写作“DX 远征”“DX 通联”，这是中文业余无线电的通行写法。

---

## C. 界面固定译法

| 英文 | 中文 | 行数 |
|---|---|---:|
| Station / rig / radio | 电台 | 712 |
| Callsign | 呼号 | 425 |
| Band | 波段 | 377 |
| Mode | 模式 | 297 |
| Settings | 设置 | 443 |
| Contact / QSO（叙述中） | 通联 | 471 |
| Logbook | 日志簿 | 89 |
| Waterfall | 瀑布图 | 60 |
| Panadapter | 全景 | 39 |
| Contest | 竞赛 | 197 |
| Activation | 激活 | 85 |
| Split | 异频 | — |
| Fake It | 伪异频 | — |
| Passband | 通带 | — |
| Notch | 陷波 | — |
| Squelch | 静噪 | — |
| Speech compressor | 语音压缩 | — |
| Drive | 驱动力 | — |
| Turn Cue | 轮次提示 | — |
| Voice Keyer | 语音键控 | — |
| Copilot | 副驾 | — |
| Roam | 漫游 | — |
| rig scope | 电台频谱 | — |
| cockpit | 操作台 | — |
| Phone（模式） | 话音 | — |
| Slant | 倾斜校正 | — |
| Crisp / Banner | 清晰 / 横幅 | — |

---

## D. 硬性规则（违反会让程序出错，`verify-zh.py` 会拦）

1. **占位符 `{{...}}`**：名字（`{{count}}`、`{{version}}`、`{{call}}`…）原样保留，出现次数必须与英文一致。位置可以移动（中文语序不同，占位符就是为此存在的），但**不能改名、不能丢、不能凭空多出来**。
2. **格式标记 `<b></b>` `<em></em>` `<code></code>` `<note></note>`**：必须成对闭合，且与英文标记集合一致。标记只是**名字**，不是 HTML：调用点没传入的元素不会渲染，多写一个就会在屏幕上原样显示成 `<b>`。
3. **技术数字保留小数点**：`14.074`、`2.4 kHz`、`45.45 baud`。频率里出现逗号会让电台频点出错——这不是口味问题，是操作事故。
4. **CW 宏 `{NAME}` `{MYCALL}` `{RST}` `{MYSTATE}`**：单花括号、全大写，原样保留。
5. **复数行**：`::one` / `::few` / `::many` / `::other` 四行都要填。中文没有单复数变化，四行通常写同一句、统一用 `{{count}}`；转换器只取 `other` 一行，其余三行是给后期维护者看的记录。

---

## 如果我认为某个词该换个译法

改 CSV 的那一列，跑一次 `python3 verify-zh.py nexus-zh-translation.csv`，再重新生成 `zh.ts`。如果改的是 `B` 节里的词（比如决定 `SWR` 保留拉丁字母），把它从 `verify-zh.py` 的 `TRANSLATED` 集合里删掉，同时改本文件——两处必须一起改，否则校验器会继续放过它。
