import { LoginForm } from "@/components/auth/login-form";

export const metadata = { title: "Sign in" };

export default async function LoginPage({
  searchParams,
}: {
  searchParams: Promise<{ next?: string; error?: string }>;
}) {
  const params = await searchParams;
  const nextPath = params.next?.startsWith("/") && !params.next.startsWith("//") ? params.next : "/dashboard";
  return (
    <LoginForm
      nextPath={nextPath}
      initialError={params.error === "session" ? "Your session expired. Sign in again to continue." : undefined}
    />
  );
}
