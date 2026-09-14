import { createApp } from 'vue'
import { createPinia } from 'pinia'
import './style.css'
import App from './App.vue'
import { router } from './router'
import { ensureDevToken } from './dev-auth'

async function bootstrap() {
  await ensureDevToken()
  createApp(App).use(createPinia()).use(router).mount('#app')
}

bootstrap()
