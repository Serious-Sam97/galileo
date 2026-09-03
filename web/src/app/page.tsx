"use client";

import { useEffect } from "react";
import { useRouter } from "next/navigation";
import { useMe } from "@/lib/hooks";

export default function Home() {
  const me = useMe();
  const router = useRouter();
  useEffect(() => {
    if (me.isError) router.replace("/login");
    else if (me.data) {
      const p = me.data.projects[0];
      router.replace(p ? `/p/${p.id}/overview` : "/login");
    }
  }, [me.data, me.isError, router]);
  return <div className="flex flex-1 items-center justify-center text-muted">Loading…</div>;
}
