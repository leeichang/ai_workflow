# 字型

字型檔本身不進版控（見根目錄 `.gitignore`），需另行下載放置於此目錄。

## 已使用的字型

| 檔名 | 大小 | 用途 | 授權 |
|---|---|---|---|
| `NotoSansCJKtc-Regular.otf` | 15.7 MB | 內文、明細 | SIL Open Font License 1.1 |
| `NotoSansCJKtc-Bold.otf` | 16.2 MB | 表頭、合計列、標題 | 同上 |

注意檔名是 `NotoSansCJKtc-*`，不是 `NotoSansTC-*`。後者在 noto-cjk repo 中不存在，會下載到 HTML 錯誤頁。

## 下載

```bash
cd pdfme/assets/fonts
for w in Regular Bold; do
  curl -sL -o "NotoSansCJKtc-$w.otf" \
    "https://github.com/notofonts/noto-cjk/raw/main/Sans/OTF/TraditionalChinese/NotoSansCJKtc-$w.otf"
done
file NotoSansCJKtc-Regular.otf   # 應顯示 OpenType font data
```

最後一行是必要的驗證。若顯示 `HTML document text` 表示路徑錯誤下載到錯誤頁。

## 實測結果（2026-09-14）

以 `poc/pdfme-font-test/test.mjs` 實測，45 行明細加 3 列合計：

| 項目 | 結果 |
|---|---|
| 中文顯示 | 正確，無豆腐字 |
| 自動分頁 | 正確，產生 2 頁 |
| `repeatHead: true` | **生效**，第 2 頁有完整表頭 |
| `columnStyles.alignment` | **生效**，逐欄可設定 |
| 合計列 | 正常呈現於末尾 |
| 檔案大小 | 128 KB（已 subsetting） |

兩個 OTF 共 32 MB，但 subsetting 後輸出的 PDF 僅 128 KB。

## 註冊方式

```javascript
const font = {
  tc:      { data: fs.readFileSync(`${F}/NotoSansCJKtc-Regular.otf`), fallback: true },
  'tc-bd': { data: fs.readFileSync(`${F}/NotoSansCJKtc-Bold.otf`) },
};
```

`fallback: true` 必須設在其中一個字型，否則未指定 `fontName` 的欄位會用預設 Roboto 而出現豆腐字。

## 注意

- 字型隨 Docker 映像檔打包，執行期不從外部下載。
- Designer 與 Generator 必須註冊同一份字型，否則設計時正常、產生的 PDF 出現豆腐字。
- 預設啟用 subsetting，只嵌入實際用到的字元。
- SIL OFL 允許商業使用與嵌入 PDF，但不得單獨販售字型本身。散布時需保留授權聲明。
