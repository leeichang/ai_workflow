<script setup lang="ts">
/**
 * 登入頁
 *
 * 不使用 AppShell——未登入時側邊欄的功能選單一個都點不了，
 * 顯示出來只是誤導。
 *
 * 租戶代碼要使用者自己填：系統是多租戶的，同一個 email 可能
 * 存在於不同租戶。後端也是以 (tenant_code, email) 為查詢鍵。
 */
import { ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useSession } from '@/auth/useSession'
import { ApiError } from '@/api/types'

const router = useRouter()
const route = useRoute()
const session = useSession()

const tenantCode = ref('')
const email = ref('')
const password = ref('')
const errorMessage = ref('')
const submitting = ref(false)

/**
 * 登入後要去哪裡
 *
 * 被 router guard 擋下來的使用者，原本要去的路徑存在 query.redirect。
 * 直接開登入頁的則進首頁。
 */
function destination(): string {
  const redirect = route.query.redirect
  if (typeof redirect === 'string' && redirect.startsWith('/')) {
    // 僅接受站內相對路徑：外部網址會變成開放重導向，
    // 攻擊者可用一條看似正常的登入連結把人導到釣魚站。
    return redirect
  }
  return '/'
}

async function submit(): Promise<void> {
  if (submitting.value) return

  errorMessage.value = ''
  submitting.value = true

  try {
    await session.login({
      tenant_code: tenantCode.value.trim(),
      email: email.value.trim(),
      password: password.value,
    })
    await router.push(destination())
  } catch (error) {
    // 後端對「查無此租戶」「查無此人」「密碼錯」一律回同一句
    // 「帳號或密碼錯誤」，前端照實顯示即可，不要自行細分。
    errorMessage.value =
      error instanceof ApiError
        ? error.message
        : '無法連線到伺服器，請稍後再試'
    // 只清密碼：租戶與帳號多半是對的，全部清掉會讓使用者重打一次
    password.value = ''
  } finally {
    submitting.value = false
  }
}
</script>

<template>
  <div class="min-h-screen bg-background flex items-center justify-center p-space-lg">
    <div class="w-full max-w-[400px]">
      <div class="flex items-center gap-space-sm mb-space-xl">
        <div
          class="w-10 h-10 rounded-lg bg-primary-container flex items-center justify-center text-on-primary font-title-md shrink-0"
        >
          M
        </div>
        <div class="flex flex-col min-w-0">
          <span class="font-title-md text-title-md text-on-surface leading-none">
            台製精工 ERP
          </span>
          <span class="font-label-caption text-label-caption text-secondary mt-0.5">
            製造營運管理系統
          </span>
        </div>
      </div>

      <form
        class="bg-surface-container-lowest border border-outline-variant rounded-xl p-space-xl"
        data-testid="login-form"
        @submit.prevent="submit"
      >
        <h1 class="font-title-md text-title-md text-on-surface mb-space-lg">登入</h1>

        <label class="block mb-space-md">
          <span class="font-label-header text-label-header text-secondary block mb-1">
            公司代碼
          </span>
          <input
            v-model="tenantCode"
            type="text"
            required
            autocomplete="organization"
            placeholder="demo"
            data-testid="login-tenant"
            class="w-full h-9 px-3 bg-surface-container-low border border-outline-variant rounded-lg font-body-dense text-body-dense text-on-surface placeholder:text-outline focus:outline-none focus:border-primary-container focus:bg-surface-container-lowest transition-all"
          />
        </label>

        <label class="block mb-space-md">
          <span class="font-label-header text-label-header text-secondary block mb-1">
            電子郵件
          </span>
          <input
            v-model="email"
            type="email"
            required
            autocomplete="username"
            placeholder="you@company.com"
            data-testid="login-email"
            class="w-full h-9 px-3 bg-surface-container-low border border-outline-variant rounded-lg font-body-dense text-body-dense text-on-surface placeholder:text-outline focus:outline-none focus:border-primary-container focus:bg-surface-container-lowest transition-all"
          />
        </label>

        <label class="block mb-space-lg">
          <span class="font-label-header text-label-header text-secondary block mb-1">
            密碼
          </span>
          <input
            v-model="password"
            type="password"
            required
            autocomplete="current-password"
            data-testid="login-password"
            class="w-full h-9 px-3 bg-surface-container-low border border-outline-variant rounded-lg font-body-dense text-body-dense text-on-surface focus:outline-none focus:border-primary-container focus:bg-surface-container-lowest transition-all"
          />
        </label>

        <p
          v-if="errorMessage"
          role="alert"
          data-testid="login-error"
          class="mb-space-md px-3 py-2 rounded-lg bg-error-container text-on-error-container font-body-dense text-body-dense"
        >
          {{ errorMessage }}
        </p>

        <button
          type="submit"
          :disabled="submitting"
          data-testid="login-submit"
          class="w-full h-9 rounded-lg bg-primary text-on-primary font-label-header text-label-header hover:opacity-90 disabled:opacity-50 disabled:cursor-not-allowed transition-opacity"
        >
          {{ submitting ? '登入中…' : '登入' }}
        </button>
      </form>

      <p class="mt-space-md text-center font-label-caption text-label-caption text-secondary">
        v0.1.0（開發版）
      </p>
    </div>
  </div>
</template>
