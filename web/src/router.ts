import { createRouter, createWebHistory } from 'vue-router'

/**
 * 路由
 *
 * 畫面依 docs/UI 設計 的設計稿實作。
 */
export const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: '/', redirect: '/designer/forms/quotation_form' },
    {
      path: '/designer/forms/:formKey',
      name: 'form-designer',
      component: () => import('./pages/FormDesigner.vue'),
    },
    {
      path: '/designer/permissions',
      name: 'permission-matrix',
      component: () => import('./pages/PermissionMatrix.vue'),
    },
    { path: '/:pathMatch(.*)*', redirect: '/designer/forms/quotation_form' },
  ],
})
