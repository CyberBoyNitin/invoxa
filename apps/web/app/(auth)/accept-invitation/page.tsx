import { AcceptInvitation } from "@/components/auth/accept-invitation";

export const metadata = { title: "Accept invitation" };

export default async function AcceptInvitationPage({
  searchParams,
}: {
  searchParams: Promise<{ token?: string }>;
}) {
  const params = await searchParams;
  return <AcceptInvitation token={params.token ?? ""} />;
}
