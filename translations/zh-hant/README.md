# Nexus 繁體中文（台灣用語）本地化套件

Nexus 是給業餘無線電用的程式：CAT 控制、FT8/CW/SSB/RTTY 收發、日誌、地圖、傳播與衛星。

這份稿子是簡體稿（`translations/zh/`）的繁體版，把用詞換成台灣的講法，不是逐句重譯。程式目前支援
英語、德語、西班牙語、法語、日語、俄語與簡體中文；加上繁體之後，用繁體介面的操作員不必再讀簡體。

註冊的語言代號是 **`zh-hant`**，語言選單裡顯示「繁體中文」。代號全部小寫是硬性要求：
`placeholders.test.ts`、`locale-switch.test.ts`、`splash.test.ts` 都用
`installCatalog\(\s*'([a-z-]+)'` 找已安裝的語言，寫成 `zh-Hant` 會被這三道檢查默默跳過。

---

## 檔案

| 檔案 | 用途 |
|---|---|
| `nexus-zh-hant-translation.csv` | 11,157 列，欄位與簡體套件相同，只填 `chinese` 欄 |
| `splash-zh-hant.json` | 啟動頁的 5 條文字，貼進 `ui/public/splashscreen.html` 的 `splash-strings` |

`notes` 欄大多是空的：這份稿子是從已完成的簡體稿轉出來的，把 3,710 條英文註解搬過來只是雜訊。維護者為新增文案補譯、等待譯者複核的 573 列，`notes` 欄寫著 “Filled by the maintainer, pending translator review.”。
要對照原文時看 `english` 欄就夠。

## 重建

改完 CSV 之後：

```
python3 translations/zh/csv-to-catalog.py translations/zh-hant/nexus-zh-hant-translation.csv \
    -o ui/src/i18n/zh-hant.ts --variant hant --splash-out translations/zh-hant/splash-zh-hant.json
```

轉換器與簡體套件共用一支（兩份 CSV 的 `key`、`english`、`do_not_translate` 三欄完全相同）。它會拒絕
欄位不對、重複的 key、只填一半的複數或啟動頁文字；英文已改或已刪的列會被跳過並報出條數。接著在
`ui/` 跑 `npx vitest run src/i18n/placeholders.test.ts src/i18n/splash.test.ts src/i18n/locale-switch.test.ts src/features/macroSets.test.ts`，
檢查佔位符與標記、數字裡的逗號、啟動頁文字，以及每個 key 是否都有譯文。沒有程式逐列核對
`do_not_translate` 欄，請人工對照。

## 轉換方式

OpenCC 的 `s2twp`（簡體轉繁體、台灣詞彙）打底，再蓋一張人工詞表。會有這張表，是因為 `s2twp`
在一批軟體與無線電術語上會挑到中國用語，偶爾也會挑錯字（`发` 轉成「髮」而不是「發」）。

| 簡體 | 這裡用 | 說明 |
|---|---|---|
| 端口 / 串口 | 連接埠 / 串列埠 | `s2twp` 只給「埠」 |
| 选项卡 / 复选框 / 对话框 | 索引標籤 / 核取方塊 / 對話方塊 | Windows 台灣用語 |
| 程序 / 进程 | 程式 / 處理程序 | |
| 高级 / 自定义 / 重置 | 進階 / 自訂 / 重設 | |
| 列表 / 当前 / 实时 | 清單 / 目前 / 即時 | |
| 双击 / 单击 / 滚动 / 拖动 | 按兩下 / 按一下 / 捲動 / 拖曳 | |
| 撤销 | 復原 | 撤銷授權時仍用「撤銷」 |
| 噪声 / 信号 / 耀斑 | 雜訊 / 訊號 / 閃焰 | 無線電與太空天氣用語 |
| 信道 / 频差 | 頻道 / 差頻 | |
| 分区 | 分區 | `s2twp` 會給「分割槽」，那是磁碟分割 |
| 条通联 / 个通联 / 个 QSO | 筆通聯 / 筆 QSO | 台灣用「筆」數通聯 |

標點也換了：簡體稿的 `“”` 全部改成 `「」`。

### 台灣用語不只換詞

上面的換字表只解決詞彙。台灣讀者一眼看出不對的地方，通常是句式與語氣，所以轉完要再讀一遍：

- **詞彙**：除了上表，常見的還有 `程式`、`設定`、`儲存`、`預設`、`網路`、`資訊`、`檔案`、`搜尋`、`支援`、`使用者`、`連線`、`帳號`。看到「程序／保存／默认／网络／信息／文件／搜索／支持／用户／连接」就換掉。
- **句式**：台灣軟體介面句子短、講得直，少用「進行…」「對…加以…」這種動詞堆疊，也不把名詞串成一長串定語。一句能拆成兩句就拆。
- **語氣**：用「你」，不用「您」；祈使句帶「請」，否定句用「無法」「尚未」，不要硬譯成「不能使用」。像「尚未設定」「無法連線」「請先選擇裝置」才是這裡的講法。
- **量詞**：一「筆」通聯、一「組」參數、一「顆」衛星。

判斷方法只有一個：念出聲。台灣的操作員不會這樣講話，就重寫，哪怕意思沒錯。

## 沒動的地方

`key`、`english`、`do_not_translate` 三欄一個字都不能改，`{{slot}}`、`<b>`、`{MACRO}` 也照原樣。
頻率、訊號報告、呼號、網格、波段與模式名稱、ADIF 欄位、獎狀名稱一律不譯。

表格的「行／列」照原文：簡體稿用「行」指 row、「列」指 column，台灣正好相反。整批對調會撞到
「執行」「進行」「放行」這些常用詞，所以只改了「欄標題」「欄寬」兩處。

---

## 致謝與授權

英文原文、術語約定與檢查腳本的形式來自上游 `translations/ru`（loloka）與 `translations/pt-BR` 兩套；繁體中文譯文由 **BG5EQA**（業餘無線電呼號，GitHub [@kmno4891](https://github.com/kmno4891)）整理，借助 AI 助理逐條校對。譯文全部經人工複核，術語取捨見 [`GLOSSARY.md`](../zh/GLOSSARY.md)。

譯文與套件腳本隨 Nexus 一同以 **GPL-3.0-only** 發布。
