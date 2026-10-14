import { StrictMode, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { NinevehProvider } from '@nineveh/react'

import { Dashboard } from './dashboard'
import { Feed } from './feed'
import { Holders } from './holders'
import { Leaderboard } from './leaderboard'
import './styles.css'

const PROJECT = import.meta.env.VITE_NINEVEH_PROJECT ?? 'http://127.0.0.1:4000/projects/market'
const KEY = import.meta.env.VITE_NINEVEH_KEY ?? ''

const EXAMPLES = [
  { name: 'Leaderboard', render: () => <Leaderboard /> },
  { name: 'Holders', render: () => <Holders /> },
  { name: 'Feed', render: () => <Feed /> },
  { name: 'Dashboard', render: () => <Dashboard /> },
]

function App() {
  const [showing, setShowing] = useState(0)
  return (
    <main>
      <nav>
        <h1>@nineveh/react</h1>
        <div className="tabs">
          {EXAMPLES.map((example, i) => (
            <button
              key={example.name}
              className={i === showing ? 'on' : undefined}
              onClick={() => setShowing(i)}
            >
              {example.name}
            </button>
          ))}
        </div>
        <code className="where">{PROJECT.replace(/^https?:\/\//, '')}</code>
      </nav>
      {EXAMPLES[showing]?.render()}
      <footer>
        Four pages against one project. Each is one file in <code>src/</code> — open the
        one you are looking at.
      </footer>
    </main>
  )
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    {/* A key is required against a hosted plane and optional against a local one, which
        is why this passes it only when there is one to pass. */}
    <NinevehProvider project={PROJECT} {...(KEY === '' ? {} : { apiKey: KEY })}>
      <App />
    </NinevehProvider>
  </StrictMode>,
)
