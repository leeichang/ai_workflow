import { createApp } from 'vue'
import { createPinia } from 'pinia'
import './style.css'
import App from './App.vue'
import { router } from './router'
import { useSession } from './auth/useSession'

/**
 * 啟動應用
 *
 * 掛載前先回復登入狀態，否則 router guard 會在 session 讀出來之前
 * 就先判定未登入，重新整理時已登入的使用者會被踢回登入頁。
 *
 * restore() 只讀 localStorage，不打網路，不會拖慢啟動。
 */
function bootstrap() {
  useSession().restore()
  createApp(App).use(createPinia()).use(router).mount('#app')
}

bootstrap()
