#!/usr/bin/env python3
"""Generate the harder relevance set `fixtures/eval-hard/` (T211).

Deterministic (no randomness beyond a fixed seed), fictional content only. Families that
make retrieval hard on purpose:

- **series**: twelve monthly bills per utility, meeting notes per client and topic — the
  model sees near-identical texts, only a month or a name tells them apart;
- **near-duplicates**: draft / v2 / final versions of the same report;
- **long documents**: handbooks where the relevant passage is one section of many;
- **same function, several languages**: retry / debounce / pagination / LRU cache in
  Python, Go, TypeScript and Rust;
- **noise**: logs, CSV exports, configs, changelogs that share vocabulary with queries;
- **bilingual**: English and Spanish documents and queries, crossing languages.

Judgments are graded: `relevant` (grade 2, the answer) and `related` (grade 1, acceptable
but not what was asked). Run from the repository root:

    python scripts/eval/make_hard_set.py
"""

import json
import os
import random
import shutil
import textwrap

ROOT = os.path.join("fixtures", "eval-hard")
CORPUS = os.path.join(ROOT, "corpus")
rng = random.Random(211)
docs = {}
queries = []

MONTHS_EN = ["January", "February", "March", "April", "May", "June", "July", "August",
             "September", "October", "November", "December"]
MONTHS_ES = ["enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto",
             "septiembre", "octubre", "noviembre", "diciembre"]


def doc(path, text):
    assert path not in docs, path
    docs[path] = textwrap.dedent(text).strip() + "\n"


def q(text, category, relevant, related=()):
    for p in list(relevant) + list(related):
        assert p in docs or p.rstrip("/") in {os.path.dirname(d) for d in docs}, p
    queries.append({"q": text, "category": category, "relevant": list(relevant),
                    "related": list(related)})


# --- utility bills: 4 utilities x 12 months -----------------------------------------
UTILITIES = [
    ("electricity", "Northwind Energy", "kWh", (210, 420), 0.19, "electricity"),
    ("gas", "Meridian Gas", "kWh", (150, 1300), 0.071, "gas"),
    ("water", "AquaCity Water", "m3", (6, 19), 2.1, "water"),
    ("internet", "FiberNet", "GB", (180, 900), 0.0, "broadband"),
]
for key, company, unit, (lo, hi), price, word in UTILITIES:
    for m in range(12):
        used = rng.randint(lo, hi)
        amount = round(used * price + rng.uniform(8, 20), 2) if price else 39.90
        path = f"finance/bills/{key}/{key}-2025-{m + 1:02d}.txt"
        doc(path, f"""
            {company} - {word} bill
            Statement for {MONTHS_EN[m]} 2025 (account 7741-{key[:2].upper()}{m + 1:02d})
            Usage this period: {used} {unit}
            Amount due: {amount:.2f} EUR, collected by direct debit on the 15th of the following month.
            {"Tip: shorter showers save water and energy." if key == "water" else ""}
            {"Your plan includes unlimited data; speeds up to 600 Mbps." if key == "internet" else ""}
            """)

q("electricity bill March 2025", "series", ["finance/bills/electricity/electricity-2025-03.txt"],
  [f"finance/bills/electricity/electricity-2025-{m:02d}.txt" for m in (2, 4)])
q("factura de la luz de marzo", "series-multilingual",
  ["finance/bills/electricity/electricity-2025-03.txt"])
q("gas bill for January", "series", ["finance/bills/gas/gas-2025-01.txt"])
q("factura del gas de diciembre", "series-multilingual", ["finance/bills/gas/gas-2025-12.txt"])
q("water bill August", "series", ["finance/bills/water/water-2025-08.txt"])
q("recibo del agua de agosto", "series-multilingual", ["finance/bills/water/water-2025-08.txt"])
q("FiberNet broadband statement October", "series", ["finance/bills/internet/internet-2025-10.txt"])
q("internet bills", "ambiguous",
  [f"finance/bills/internet/internet-2025-{m:02d}.txt" for m in range(1, 13)])

# --- client meetings: 6 clients x 3 topics ------------------------------------------
CLIENTS = ["Acme Logistics", "Globex", "Initech", "Umbrella Health", "Stark Freight", "Wayne Retail"]
TOPICS = {
    "kickoff": ("Kickoff meeting", "agreed the project scope, the weekly status call and the "
                "first milestone: data import working by the end of the month."),
    "pricing": ("Pricing discussion", "they asked for a volume discount; we offered tiers by "
                "number of seats and a two-year commitment instead of a flat discount."),
    "renewal": ("Contract renewal", "renewal for another year is likely; open points are single "
                "sign-on and the termination clause, legal comments due Friday."),
}
for c in CLIENTS:
    slug = c.lower().split()[0]
    for t, (title, body) in TOPICS.items():
        doc(f"work/clients/{slug}/{t}-notes.md", f"""
            # {c} - {title}

            Attendees: account team, {c} operations lead.

            Summary: {body}

            Next steps: send minutes, update the CRM, follow up next week.
            """)
# Spanish notes for two clients
doc("work/clients/initech/reunion-incidencia.md", """
    # Initech - reunión sobre la incidencia del lunes

    El servicio de facturación estuvo caído dos horas. Causa: certificado TLS caducado en el
    balanceador. Acciones: renovar con antelación automática y alerta 30 días antes.
    """)
doc("work/clients/globex/reunion-precios.md", """
    # Globex - segunda reunión de precios

    Piden un descuento del 8 %. Proponemos precio por tramos y soporte prioritario incluido.
    Decisión final de su director financiero la semana que viene.
    """)
q("Globex pricing discussion", "series", ["work/clients/globex/pricing-notes.md",
                                          "work/clients/globex/reunion-precios.md"],
  ["work/clients/globex/kickoff-notes.md"])
q("Initech contract renewal notes", "series", ["work/clients/initech/renewal-notes.md"])
q("reunión con Umbrella sobre la renovación", "series-multilingual",
  ["work/clients/umbrella/renewal-notes.md"])
q("kickoff with Stark Freight", "series", ["work/clients/stark/kickoff-notes.md"])
q("which client had an expired certificate outage", "paraphrase",
  ["work/clients/initech/reunion-incidencia.md"])
q("Wayne Retail volume discount", "series", ["work/clients/wayne/pricing-notes.md"])

# --- near-duplicate report versions -------------------------------------------------
REPORT = """
    # Annual sustainability report {ver}

    Emissions fell {pct} % compared with last year, mostly from moving the data centre to
    renewable energy. Business travel is now the largest source. {extra}
    """
doc("work/reports/sustainability-report-draft.md",
    REPORT.format(ver="(draft)", pct="9", extra="TODO: check the travel figures."))
doc("work/reports/sustainability-report-v2.md",
    REPORT.format(ver="(v2, comments from finance)", pct="11",
                  extra="Travel figures checked; still missing the supplier survey."))
doc("work/reports/sustainability-report-final.md",
    REPORT.format(ver="(final, approved by the board)", pct="12",
                  extra="Supplier survey included. Approved for publication on 3 March."))
q("final approved sustainability report", "near-duplicate",
  ["work/reports/sustainability-report-final.md"],
  ["work/reports/sustainability-report-v2.md", "work/reports/sustainability-report-draft.md"])
q("draft of the emissions report with TODOs", "near-duplicate",
  ["work/reports/sustainability-report-draft.md"],
  ["work/reports/sustainability-report-v2.md"])

# --- long documents: handbook with many sections ------------------------------------
SECTIONS = [
    ("Working hours", "Core hours are 10:00-16:00; the rest of the day is flexible."),
    ("Remote work", "Employees may work remotely up to three days a week with their team's "
                    "agreement; the company pays for a monitor and a chair."),
    ("Holidays", "Twenty-five days of paid holiday per year, plus public holidays. Up to five "
                 "unused days carry over to March."),
    ("Sick leave", "Tell your manager before 10:00. A doctor's note is needed after three days."),
    ("Parental leave", "Sixteen weeks fully paid for every parent, which can be split into "
                       "blocks during the child's first year."),
    ("Expenses", "Submit receipts within thirty days; travel is booked through the travel desk."),
    ("Equipment", "Laptops are replaced every three years; report a lost device at once."),
    ("Training", "Each employee has a yearly training budget of 1,500 EUR."),
    ("Security", "Use the password manager and enable two-factor authentication everywhere."),
    ("Leaving the company", "The notice period is one month in the first year, two after that."),
]
body = "\n\n".join(f"## {i + 1}. {t}\n\n{txt} " + " ".join(
    f"Further details on {t.lower()} are in the intranet page, revision {r}." for r in range(4))
    for i, (t, txt) in enumerate(SECTIONS))
doc("work/handbook/employee-handbook.md", "# Employee handbook\n\n" + body)
q("parental leave policy", "long-document", ["work/handbook/employee-handbook.md"])
q("how many days can I work from home", "long-document", ["work/handbook/employee-handbook.md"])
q("training budget per year", "long-document", ["work/handbook/employee-handbook.md"])
q("notice period when resigning", "long-document", ["work/handbook/employee-handbook.md"])

LONG_ES = [
    ("Instalación", "Descargar el instalador, ejecutar como administrador y reiniciar."),
    ("Primeros pasos", "Crear un proyecto nuevo desde el menú Archivo."),
    ("Copias de seguridad", "Las copias se guardan cada noche en la carpeta de respaldo; "
                            "conservar al menos siete días."),
    ("Exportar a PDF", "Menú Archivo, Exportar, elegir PDF y la calidad de imagen."),
    ("Atajos de teclado", "Ctrl+S guarda, Ctrl+P imprime, Ctrl+Mayús+E exporta."),
    ("Problemas frecuentes", "Si el programa no arranca, borrar la caché de la carpeta temporal."),
]
doc("study/manuales/manual-usuario.md", "# Manual de usuario\n\n" + "\n\n".join(
    f"## {t}\n\n{txt}" for t, txt in LONG_ES))
q("cómo exportar a PDF", "long-document", ["study/manuales/manual-usuario.md"])
q("backup retention days in the user manual", "long-document-multilingual",
  ["study/manuales/manual-usuario.md"])

# --- code: same function in several languages ---------------------------------------
CODE = {
    "retry": {
        "py": '"""Retry an HTTP GET with exponential backoff."""\n\nimport time\nimport requests\n\n'
              'def get_with_retry(url, attempts=4):\n    for i in range(attempts):\n        try:\n'
              '            return requests.get(url, timeout=5)\n        except requests.RequestException:\n'
              '            time.sleep(2 ** i)\n    raise RuntimeError("failed")\n',
        "go": "package net\n\n// GetWithRetry retries a GET with exponential backoff.\nfunc GetWithRetry(url string, "
              "attempts int) (*http.Response, error) {\n\tvar err error\n\tfor i := 0; i < attempts; i++ {\n"
              "\t\tresp, e := http.Get(url)\n\t\tif e == nil {\n\t\t\treturn resp, nil\n\t\t}\n\t\terr = e\n"
              "\t\ttime.Sleep(time.Duration(1<<i) * time.Second)\n\t}\n\treturn nil, err\n}\n",
        "ts": "/** Retries fetch with exponential backoff. */\nexport async function fetchWithRetry(url: string, "
              "attempts = 4): Promise<Response> {\n  for (let i = 0; i < attempts; i++) {\n    try {\n"
              "      return await fetch(url);\n    } catch {\n      await new Promise((r) => setTimeout(r, 2 ** i * 1000));\n"
              "    }\n  }\n  throw new Error(\"failed\");\n}\n",
        "rs": "/// Retries a blocking GET with exponential backoff.\npub fn get_with_retry(url: &str, attempts: u32) "
              "-> Result<String, String> {\n    for i in 0..attempts {\n        match ureq::get(url).call() {\n"
              "            Ok(r) => return r.into_string().map_err(|e| e.to_string()),\n"
              "            Err(_) => std::thread::sleep(std::time::Duration::from_secs(1 << i)),\n        }\n    }\n"
              "    Err(\"failed\".into())\n}\n",
    },
    "paginate": {
        "py": '"""Paginate a list endpoint by cursor."""\n\ndef fetch_all(client, path):\n    items, cursor = [], None\n'
              '    while True:\n        page = client.get(path, params={"cursor": cursor})\n        items += page["items"]\n'
              '        cursor = page.get("next")\n        if not cursor:\n            return items\n',
        "ts": "/** Walks every page of a cursor-paginated API. */\nexport async function fetchAll<T>(get: "
              "(cursor?: string) => Promise<{ items: T[]; next?: string }>): Promise<T[]> {\n  const out: T[] = [];\n"
              "  let cursor: string | undefined;\n  do {\n    const page = await get(cursor);\n    out.push(...page.items);\n"
              "    cursor = page.next;\n  } while (cursor);\n  return out;\n}\n",
    },
    "lru": {
        "py": '"""A small least-recently-used cache."""\n\nfrom collections import OrderedDict\n\nclass LruCache:\n'
              '    def __init__(self, capacity):\n        self.capacity = capacity\n        self.data = OrderedDict()\n\n'
              '    def get(self, key):\n        if key not in self.data:\n            return None\n'
              '        self.data.move_to_end(key)\n        return self.data[key]\n\n    def put(self, key, value):\n'
              '        self.data[key] = value\n        self.data.move_to_end(key)\n        if len(self.data) > self.capacity:\n'
              '            self.data.popitem(last=False)\n',
        "go": "package cache\n\n// LRU evicts the least recently used entry when full.\ntype LRU struct {\n\tcap   int\n"
              "\torder *list.List\n\titems map[string]*list.Element\n}\n",
        "rs": "/// Least-recently-used cache with a fixed capacity.\npub struct Lru<K, V> {\n    capacity: usize,\n"
              "    map: HashMap<K, V>,\n    order: VecDeque<K>,\n}\n",
    },
    "debounce": {
        "ts": "/** Calls `fn` only after `ms` without new calls. */\nexport function debounce<A extends unknown[]>(fn: "
              "(...a: A) => void, ms: number) {\n  let t: ReturnType<typeof setTimeout> | undefined;\n"
              "  return (...a: A) => {\n    clearTimeout(t);\n    t = setTimeout(() => fn(...a), ms);\n  };\n}\n",
        "py": '"""Debounce decorator for event handlers."""\n\nimport threading\n\ndef debounce(seconds):\n'
              '    def wrap(fn):\n        timer = None\n        def call(*args):\n            nonlocal timer\n'
              '            if timer:\n                timer.cancel()\n            timer = threading.Timer(seconds, fn, args)\n'
              '            timer.start()\n        return call\n    return wrap\n',
    },
    "csv": {
        "py": '"""Read a CSV export into dictionaries, skipping blank lines."""\n\nimport csv\n\ndef read_rows(path):\n'
              '    with open(path, newline="", encoding="utf-8") as f:\n'
              '        return [row for row in csv.DictReader(f) if any(row.values())]\n',
        "rs": "/// Parses a CSV file into rows of strings.\npub fn read_rows(path: &Path) -> csv::Result<Vec<Vec<String>>> {\n"
              "    let mut r = csv::Reader::from_path(path)?;\n    r.records().map(|rec| rec.map(|r| r.iter()"
              ".map(str::to_owned).collect())).collect()\n}\n",
    },
}
EXT_DIR = {"py": "python", "go": "go", "ts": "typescript", "rs": "rust"}
for fn, impls in CODE.items():
    for ext, src in impls.items():
        doc(f"code/{EXT_DIR[ext]}/{fn}.{ext}", src)
q("retry http request in go", "code", ["code/go/retry.go"],
  ["code/python/retry.py", "code/typescript/retry.ts", "code/rust/retry.rs"])
q("exponential backoff python", "code", ["code/python/retry.py"], ["code/go/retry.go"])
q("cursor pagination typescript", "code", ["code/typescript/paginate.ts"], ["code/python/paginate.py"])
q("least recently used cache", "code", ["code/python/lru.py", "code/go/lru.go", "code/rust/lru.rs"])
q("debounce decorator in python", "code", ["code/python/debounce.py"], ["code/typescript/debounce.ts"])
q("read csv file rust", "code", ["code/rust/csv.rs"], ["code/python/csv.py"])
q("función que reintenta peticiones", "code-multilingual",
  ["code/python/retry.py", "code/go/retry.go", "code/typescript/retry.ts", "code/rust/retry.rs"])

# --- recipes, travel, study (distinct topics, bilingual) -----------------------------
RECIPES = {
    "tortilla-de-patatas.md": "# Tortilla de patatas\n\nHuevos, patatas y cebolla; freír despacio y cuajar en sartén.",
    "gazpacho.md": "# Gazpacho\n\nTomate, pepino, pimiento, ajo, pan, aceite y vinagre; triturar y servir frío.",
    "paella.md": "# Paella valenciana\n\nArroz, pollo, conejo, judía verde y azafrán; cocer 18 minutos sin remover.",
    "lentejas.md": "# Lentejas con chorizo\n\nLentejas, chorizo, zanahoria y pimentón; guisar 40 minutos.",
    "banana-bread.md": "# Banana bread\n\nRipe bananas, butter, sugar, egg and flour; bake 55 minutes at 175 C.",
    "pancakes.md": "# Pancakes\n\nFlour, milk, eggs and baking powder; cook on a hot pan until bubbles form.",
    "chili.md": "# Chili con carne\n\nMinced beef, kidney beans, tomatoes, cumin and chili; simmer one hour.",
    "risotto.md": "# Mushroom risotto\n\nArborio rice, mushrooms, stock, parmesan; stir for 18 minutes.",
    "hummus.md": "# Hummus\n\nChickpeas, tahini, lemon, garlic and olive oil; blend until smooth.",
    "brownies.md": "# Chocolate brownies\n\nDark chocolate, butter, sugar, eggs, a little flour; bake 25 minutes.",
}
for name, text in RECIPES.items():
    doc(f"recipes/{name}", text)
q("cold Spanish tomato soup", "multilingual", ["recipes/gazpacho.md"])
q("receta con garbanzos", "multilingual", ["recipes/hummus.md"])
q("rice dish with saffron", "multilingual", ["recipes/paella.md"], ["recipes/risotto.md"])
q("postre de chocolate", "multilingual", ["recipes/brownies.md"])
q("something with beans and minced meat", "paraphrase", ["recipes/chili.md"])

TRIPS = {
    "japan-2024/itinerary.md": "# Japan trip 2024\n\nTokyo four days, Kyoto temples, day trip to Nara, Osaka street food.",
    "japan-2024/packing.md": "# Packing for Japan\n\nRail pass voucher, adapter type A, light rain jacket, cash for small shops.",
    "lisbon-2025/booking.txt": "Skyline Air booking QX7P2L: Madrid-Lisbon 14 July, return 21 July. Cabin bag only.",
    "lisbon-2025/itinerary.md": "# Lisbon week\n\nAlfama, tram 28, Belém tower, a day in Sintra palaces.",
    "algarve-2023/packing.md": "# Beach week packing list\n\nSunscreen, towels, snorkel, sandals, waterproof camera.",
    "pirineos-2025/ruta.md": "# Ruta por los Pirineos\n\nTres días de senderismo: refugio de Góriz, Ordesa, cola de caballo.",
}
for name, text in TRIPS.items():
    doc(f"travel/{name}", text)
q("what to pack for the beach", "paraphrase", ["travel/algarve-2023/packing.md"],
  ["travel/japan-2024/packing.md"])
q("hiking route in the Pyrenees", "multilingual", ["travel/pirineos-2025/ruta.md"])
q("vuelo a Lisboa", "multilingual", ["travel/lisbon-2025/booking.txt"],
  ["travel/lisbon-2025/itinerary.md"])
q("temples in Kyoto", "lexical", ["travel/japan-2024/itinerary.md"])

STUDY = {
    "ml/gradient-descent.md": "# Gradient descent\n\nStep against the gradient; a learning rate too large diverges, too small is slow.",
    "ml/backpropagation.md": "# Backpropagation\n\nThe chain rule passes errors backwards through the layers.",
    "ml/attention.md": "# Attention\n\nSelf-attention lets every token look at every other token; multi-head attention.",
    "ml/overfitting.md": "# Overfitting\n\nValidation loss rises while training loss falls; use regularisation and early stopping.",
    "math/eigenvalues.md": "# Eigenvalues\n\nA v = lambda v; roots of det(A - lambda I); symmetric matrices have real eigenvalues.",
    "math/bayes.md": "# Bayes\n\nPosterior is likelihood times prior over evidence; base rates matter.",
    "historia/revolucion-francesa.md": "# Revolución francesa\n\n1789, toma de la Bastilla, la República, el Terror, Napoleón.",
    "historia/guerra-civil.md": "# Guerra civil española\n\n1936-1939; bandos republicano y sublevado; consecuencias.",
}
for name, text in STUDY.items():
    doc(f"study/{name}", text)
q("why my model does well on training data but badly on new data", "paraphrase",
  ["study/ml/overfitting.md"])
q("storming of the Bastille", "multilingual", ["study/historia/revolucion-francesa.md"])
q("learning rate too high", "paraphrase", ["study/ml/gradient-descent.md"])
q("transformer self-attention notes", "lexical", ["study/ml/attention.md"])

# --- noise: logs, csv, configs, changelogs sharing vocabulary -------------------------
for d in range(1, 21):
    lines = []
    for h in range(0, 24, 3):
        lvl = rng.choice(["INFO", "INFO", "INFO", "WARN", "ERROR"])
        msg = rng.choice(["request completed", "retry scheduled", "cache miss", "payment received",
                          "connection reset by peer", "invoice generated", "user signed in"])
        lines.append(f"2025-06-{d:02d}T{h:02d}:00:00Z {lvl} {msg} id={rng.randint(1000, 9999)}")
    doc(f"logs/app-2025-06-{d:02d}.log", "\n".join(lines))
for m in range(1, 13):
    rows = ["order_id,customer,amount,currency"] + [
        f"{m * 1000 + i},{rng.choice(CLIENTS)},{rng.randint(20, 900)}.00,EUR" for i in range(8)]
    doc(f"exports/orders-2025-{m:02d}.csv", "\n".join(rows))
for i in range(1, 11):
    doc(f"projects/service-{i}/CHANGELOG.md",
        f"# Changelog\n\n## 1.{i}.0\n\n- retry failed requests\n- faster pagination\n- fix cache invalidation")
    doc(f"projects/service-{i}/config.toml",
        f"[server]\nport = {8000 + i}\n\n[retry]\nattempts = {rng.randint(2, 6)}\nbackoff_ms = 250\n")
q("ERROR connection reset by peer", "lexical",
  [p for p, t in docs.items() if p.startswith("logs/") and "ERROR connection reset by peer" in t])
q("service-7 changelog", "exact", ["projects/service-7/CHANGELOG.md"], ["projects/service-7"])
q("orders export for May", "series", ["exports/orders-2025-05.csv"])

# --- exact names ---------------------------------------------------------------------
q("employee-handbook", "exact", ["work/handbook/employee-handbook.md"])
q("paella", "exact", ["recipes/paella.md"])
q("lentejas", "exact", ["recipes/lentejas.md"])
q("water-2025-02", "exact", ["finance/bills/water/water-2025-02.txt"])

# --- write ---------------------------------------------------------------------------
if os.path.isdir(CORPUS):
    shutil.rmtree(CORPUS)
for path, text in sorted(docs.items()):
    full = os.path.join(CORPUS, *path.split("/"))
    os.makedirs(os.path.dirname(full), exist_ok=True)
    with open(full, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)
out = {
    "description": "T211 harder relevance set (generated by scripts/eval/make_hard_set.py): series "
                   "of near-identical documents, near-duplicate versions, long documents, the same "
                   "function in several languages, noise files, English and Spanish. `relevant` = "
                   "grade 2 (the answer), `related` = grade 1. Fictional content only.",
    "version": 1,
    "queries": queries,
}
with open(os.path.join(ROOT, "queries.json"), "w", encoding="utf-8", newline="\n") as f:
    json.dump(out, f, ensure_ascii=False, indent=1)
    f.write("\n")
print(f"{len(docs)} documents, {len(queries)} queries")
