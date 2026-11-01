import { SettingsTabs } from "@/components/settings-tabs";
import { PageHeader } from "@/components/ui/page-header";

export default function SettingsLayout({ children }: { children: React.ReactNode }) {
  return (
    <div className="space-y-6">
      <PageHeader
        eyebrow="Workspace administration"
        title="Settings"
        description="Organization policy, team access, and machine integration credentials. All changes are tenant-scoped and audited."
      />
      <SettingsTabs />
      {children}
    </div>
  );
}
