const when: Record<string, string> = { Inbox: "milestone 3", Review: "milestone 4", Digest: "milestone 6" };

export default function Stub({ name }: { name: string }) {
  return (
    <div className="empty">
      <h2>{name}</h2>
      <p>Coming in {when[name] ?? "a later milestone"}.</p>
    </div>
  );
}
