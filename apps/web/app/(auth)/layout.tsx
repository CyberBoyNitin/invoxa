import Link from "next/link";

export default function AuthLayout({ children }: { children: React.ReactNode }) {
  return (
    <div className="grid min-h-screen lg:grid-cols-[0.92fr_1.08fr]">
      <aside className="relative hidden overflow-hidden bg-[var(--sidebar)] px-10 py-10 text-white lg:flex lg:flex-col">
        <div
          className="pointer-events-none absolute inset-0 opacity-[0.14]"
          style={{
            backgroundImage:
              "linear-gradient(rgba(255,255,255,.16) 1px, transparent 1px), linear-gradient(90deg, rgba(255,255,255,.16) 1px, transparent 1px)",
            backgroundSize: "64px 64px",
          }}
        />
        <div className="relative flex items-center gap-3">
          <div className="grid size-10 place-items-center rounded-md border border-white/20 bg-white/[0.07]">
            <span className="font-display text-xl font-semibold tracking-[-0.04em]">IX</span>
          </div>
          <div>
            <p className="font-display text-2xl leading-none">Invoxa</p>
            <p className="mt-1 text-[9px] uppercase tracking-[0.2em] text-[#8e94a4]">Invoice intelligence</p>
          </div>
        </div>

        <div className="relative my-auto max-w-lg py-16">
          <p className="mb-5 text-[11px] font-semibold uppercase tracking-[0.18em] text-[#9ba1b2]">
            Accounts payable, under control
          </p>
          <h1 className="font-display text-[3.35rem] leading-[1.04] tracking-[-0.035em]">
            From inbox chaos to a clean ledger.
          </h1>
          <p className="mt-6 max-w-md text-[15px] leading-7 text-[#b9bdc8]">
            Extract, validate, and review supplier invoices with an audit trail behind every decision.
            AI proposes. Your team approves.
          </p>
          <div className="mt-12 grid gap-5 border-t border-white/10 pt-8">
            {[
              ["01", "Exact, traceable extraction", "Every value keeps its source page, method, and correction history."],
              ["02", "Validation before approval", "Arithmetic, tax, required fields, and duplicate candidates are checked deterministically."],
              ["03", "No silent exports", "Only approved, valid invoices can leave the workspace."],
            ].map(([number, title, description]) => (
              <div key={number} className="flex gap-4">
                <span className="mt-0.5 font-mono text-[10px] text-[#7d8494]">{number}</span>
                <div>
                  <p className="text-sm font-semibold text-white">{title}</p>
                  <p className="mt-1 text-xs leading-5 text-[#8e94a4]">{description}</p>
                </div>
              </div>
            ))}
          </div>
        </div>
        <p className="relative text-[10px] uppercase tracking-[0.14em] text-[#6f7584]">
          Private by default · Organization-scoped
        </p>
      </aside>

      <main className="flex min-h-screen flex-col bg-paper">
        <div className="flex items-center justify-between px-6 py-5 lg:justify-end">
          <Link href="/" className="flex items-center gap-2 lg:hidden">
            <span className="grid size-8 place-items-center rounded bg-ink font-display text-sm text-white">IX</span>
            <span className="font-display text-lg">Invoxa</span>
          </Link>
          <p className="text-xs text-muted">
            Need help?{" "}
            <a className="font-semibold text-indigo hover:underline" href="mailto:support@invoxa.local">
              support@invoxa.local
            </a>
          </p>
        </div>
        <div className="flex flex-1 items-center justify-center px-5 py-10 sm:px-10">
          <div className="w-full max-w-[430px]">{children}</div>
        </div>
        <div className="px-6 py-5 text-center text-[10px] uppercase tracking-[0.12em] text-muted">
          Invoxa · Human-in-the-loop invoice processing
        </div>
      </main>
    </div>
  );
}
