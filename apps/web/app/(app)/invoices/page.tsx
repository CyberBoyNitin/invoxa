import { InvoicesList } from "@/components/invoices-list";
import { PageHeader } from "@/components/ui/page-header";

export const metadata = { title: "Invoices" };

export default function InvoicesPage() {
  return (
    <div className="space-y-6">
      <PageHeader
        eyebrow="Financial records"
        title="Invoices"
        description="Every extracted invoice with its current workflow status. Monetary values are shown from exact stored decimals."
      />
      <InvoicesList />
    </div>
  );
}
