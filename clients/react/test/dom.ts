/**
 * A DOM for the hook tests, registered before anything imports React.
 *
 * Loaded with `node --test --import ./test/dom.ts`, which runs it in every test
 * process. The tests that touch no DOM — `query`, `client`, `feed` — are unaffected
 * by it being there; React, which reads `window` as it loads, is not.
 */
import { GlobalRegistrator } from '@happy-dom/global-registrator'

GlobalRegistrator.register({ url: 'https://test.local/' })

// React only runs effects synchronously inside `act` when it believes it is under test.
;(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true
