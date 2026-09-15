import { createApp } from 'vue'
import { createPinia } from 'pinia'
import './style.css'
import App from './App.vue'
import { router } from './router'
import { ensureDevToken } from './dev-auth'

/**
 * 啟動應用
 *
 * 自動登入的失敗不可阻擋掛載。若讓錯誤往上冒，createApp 不會執行，
 * 畫面會是全白，開發者只能從 console 猜原因。
 * 掛載後至少能看到版面與錯誤提示。
 */
async function bootstrap() {
  await ensureDevToken().catch((e) => {
    console.warn('[dev] 自動登入流程異常，仍繼續掛載應用', e)
  })

  createApp(App).use(createPinia()).use(router).mount('#app')
}

void bootstrap()
