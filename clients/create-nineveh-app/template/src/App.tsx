import { useFlash, useLiveTable } from "@nineveh/react";

/**
 * __TABLE__, live.
 *
 * `useLiveTable` reads the table and holds the change feed open: when a row changes on
 * chain, this re-reads and the row flashes. That is the whole of it — delete the parts
 * you don't want.
 *
 * The columns below came from your project when this was scaffolded. `src/nineveh-tables.d.ts`
 * is generated too, which is why `row.__FIRST__` knows its type; regenerate it with
 * `npm run types` after changing your config.
 */
export default function App() {
  const { data, loading, error, connected } = useLiveTable("__TABLE__", {
    order: "_version.desc",
    limit: 25,
  });
  const flashing = useFlash(data, __KEY__);

  if (error) return <main><p className="bad">{error.message}</p></main>;

  return (
    <main>
      <header>
        <h1>__TABLE__</h1>
        <span className={connected ? "dot" : "dot off"} />
      </header>

      {loading && !data ? (
        <p className="waiting">Reading…</p>
      ) : data?.length === 0 ? (
        <p className="waiting">No rows yet. They appear as the chain writes them.</p>
      ) : (
        <table>
          <thead>
            <tr>
__HEADERS__
            </tr>
          </thead>
          <tbody>
            {data?.map((row) => (
              <tr key={__ROWKEY__} className={flashing.has(__ROWKEY__) ? "flash" : undefined}>
__CELLS__
              </tr>
            ))}
          </tbody>
        </table>
      )}

      <footer>
        Live from <a href="https://nineveh.dev">Nineveh</a>. Edit <code>src/App.tsx</code>.
      </footer>
    </main>
  );
}
