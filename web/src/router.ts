import { createRouter, createWebHistory } from 'vue-router'

/**
 * 路由
 *
 * 畫面依 docs/UI 設計 的設計稿實作。
 * 尚未實作的頁面暫時導向權限矩陣，避免 404。
 */
export const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: '/', redirect: '/designer/permissions' },
    {
      path: '/designer/permissions',
      name: 'permission-matrix',
      component: () => import('./pages/PermissionMatrix.vue'),
    },
    { path: '/:pathMatch(.*)*', redirect: '/designer/permissions' },
  ],
})
