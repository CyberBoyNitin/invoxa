import { Inbox } from "lucide-react";
import { InvoicesList } from "@/components/invoices-list";
import { PageHeader } from "@/components/ui/page-header";

export const metadata = { title: "Review queue" };

export default function ReviewQueuePage() {
  return (
    <div className="space-y-6">
      <PageHeader
        eyebrow="Human in the loop"
        title="Review queue"
        description="Exceptions only. Approve, correct, or reject after checking the original document, validation issues, and duplicate candidates."
        actions={
          <div className="flex items-center gap-2 rounded-md border border-amber/25 bg-amber-soft px-3 py-2 text-xs font-semibold text-amber">
            <Inbox className="size-3.5" />
            Only explicit decisions leave this queue
          </div>
        }
      />
      <InvoicesList initialStatus="review_required" lockStatus />
    </div>
  );
}
