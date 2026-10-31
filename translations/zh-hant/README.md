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
| `nexus-zh-hant-translation.csv` | 10,515 列，欄位與簡體套件相同，只填 `chinese` 欄 |
| `splash-zh-hant.json` | 啟動頁的 5 條文字，貼進 `ui/public/splashscreen.html` 的 `splash-strings` |

`notes` 欄是空的：這份稿子是從已完成的簡體稿轉出來的，把 3,710 條英文註解搬過來只是雜訊。
要對照原文時看 `english` 欄就夠。

## 重建

改完 CSV 之後：

```
python3 translations/zh/verify-zh.py translations/zh-hant/nexus-zh-hant-translation.csv
python3 translations/zh/csv-to-catalog.py translations/zh-hant/nexus-zh-hant-translation.csv \
    -o ui/src/i18n/zh-hant.ts --variant hant --splash-out translations/zh-hant/splash-zh-hant.json
```

檢查程式與簡體套件共用一支：兩份 CSV 的 `key`、`english`、`do_not_translate` 三欄完全相同，
所以同一組指紋對兩份都成立。

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

## 沒動的地方

`key`、`english`、`do_not_translate` 三欄一個字都不能改，`{{slot}}`、`<b>`、`{MACRO}` 也照原樣。
頻率、訊號報告、呼號、網格、波段與模式名稱、ADIF 欄位、獎狀名稱一律不譯。

表格的「行／列」照原文：簡體稿用「行」指 row、「列」指 column，台灣正好相反。整批對調會撞到
「執行」「進行」「放行」這些常用詞，所以只改了「欄標題」「欄寬」兩處。
