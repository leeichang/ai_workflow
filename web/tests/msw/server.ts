import { setupServer } from 'msw/node'

/** 無預設 handler。每個測試自行宣告需要攔截的端點。 */
export const server = setupServer()
