import { createRouter, createWebHistory } from 'vue-router'
import { installAuthGuard } from './auth/guard'

/**
 * 路由
 *
 * 畫面依 docs/UI 設計 的設計稿實作。
 *
 * 未標 meta.public 的路由都需要登入，由 auth/guard.ts 把關。
 */
export const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: '/login',
      name: 'login',
      component: () => import('./pages/LoginPage.vue'),
      meta: { public: true },
    },
    {
      path: '/',
      name: 'home',
      component: () => import('./pages/HomePage.vue'),
    },
    {
      path: '/tasks',
      name: 'tasks',
      component: () => import('./pages/TaskInbox.vue'),
    },
    {
      path: '/quotations',
      name: 'quotations',
      component: () => import('./pages/QuotationList.vue'),
    },
    {
      path: '/quotations/new',
      name: 'quotation-create',
      component: () => import('./pages/QuotationCreate.vue'),
    },
    // 選單上有、但尚未實作的功能。共用一頁說明缺什麼，
    // 而不是靜默地被 catch-all 導走——那會讓人以為選單壞了。
    {
      path: '/purchase-requests',
      name: 'purchase-requests',
      component: () => import('./pages/NotBuiltPage.vue'),
    },
    {
      path: '/reports',
      name: 'reports',
      component: () => import('./pages/NotBuiltPage.vue'),
    },
    {
      path: '/settings',
      name: 'settings',
      component: () => import('./pages/NotBuiltPage.vue'),
    },
    // 側邊欄的「設計器」指向這裡，進表單清單
    //
    // 先前 redirect 寫死成 /designer/forms/quotation_form——
    // 那讓設計器看起來只能編輯那一張既有表單，使用者無從建立新的。
    {
      path: '/designer',
      redirect: '/designer/forms',
    },
    // 清單要排在 :formKey 之前，否則 'forms' 之後的路徑段
    // 會被當成 formKey 吃掉
    {
      path: '/designer/forms',
      name: 'form-list',
      component: () => import('./pages/FormList.vue'),
    },
    {
      path: '/designer/forms/:formKey',
      name: 'form-designer',
      component: () => import('./pages/FormDesigner.vue'),
    },
    // 同樣要排在 :workflowKey 之前
    // 模擬簽核。放在 /simulation 而非 /designer 底下——
    // 它不是設計工具，是驗證工具。
    {
      path: '/simulation',
      name: 'simulation',
      component: () => import('./pages/SimulationMode.vue'),
    },
    {
      path: '/monitor',
      name: 'monitor',
      component: () => import('./pages/ProcessMonitor.vue'),
    },
    {
      path: '/designer/workflows',
      name: 'workflow-list',
      component: () => import('./pages/WorkflowList.vue'),
    },
    {
      path: '/designer/workflows/:workflowKey',
      name: 'workflow-designer',
      component: () => import('./pages/WorkflowDesigner.vue'),
    },
    {
      path: '/designer/templates/:templateKey?',
      name: 'template-designer',
      component: () => import('./pages/TemplateDesigner.vue'),
    },
    // 組織管理與健康檢查。放在設計器底下而非設定：
    // 它們回答的是「流程會不會卡」，是設計流程時要看的東西
    {
      path: '/designer/organization',
      name: 'organization',
      component: () => import('./pages/OrgManage.vue'),
    },
    {
      path: '/designer/org-import',
      name: 'org-import',
      component: () => import('./pages/OrgImport.vue'),
    },
    {
      path: '/designer/sync-center',
      name: 'sync-center',
      component: () => import('./pages/SyncCenter.vue'),
    },
    {
      path: '/designer/org-health',
      name: 'org-health',
      component: () => import('./pages/OrgHealth.vue'),
    },
    {
      path: '/designer/permissions',
      name: 'permission-matrix',
      component: () => import('./pages/PermissionMatrix.vue'),
    },
    { path: '/:pathMatch(.*)*', redirect: '/' },
  ],
})

installAuthGuard(router)
