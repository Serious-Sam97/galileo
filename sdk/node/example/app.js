// Minimal Express app: GALILEO_ENDPOINT / GALILEO_API_KEY / OTEL_SERVICE_NAME then `npm run example`.
import express from "express";
import { galileoExpress, traced, captureException, log } from "@galileo/node";

const app = express();
app.use((req, _res, next) => { req.user = req.headers["x-user"] ? { id: String(req.headers["x-user"]) } : undefined; next(); });
app.use(galileoExpress());

const priceCart = traced(async function priceCart(items) { await new Promise((r) => setTimeout(r, 20)); return items * 9.9; });

app.get("/cart/:n", async (req, res) => {
  log("info", "pricing cart", { "cart.items": Number(req.params.n) });
  res.json({ total: await priceCart(Number(req.params.n)) });
});
app.get("/boom", (_req, _res) => { throw new TypeError("cart is null"); });
app.use((err, _req, res, _next) => { captureException(err); res.status(500).json({ error: err.message }); });
app.listen(3005, () => console.log("example on :3005"));
