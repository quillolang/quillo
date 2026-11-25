# Q-1394: a chain of idempotent services posts twice after a crash in the middle

    export QUILLO_SERVICE_SECRET=$(openssl rand -hex 32)
    quillo run ledger.qwe --port 8252 --as-service
    BILLING_PAUSE=3000 quillo run billing.qwe --port 8251 --as-service --service-url ledger=http://127.0.0.1:8252
    quillo run main.qwe --port 8250 --service-url billing=http://127.0.0.1:8251 --service-url ledger=http://127.0.0.1:8252
    curl -X POST -H 'Idempotency-Key: k1' 127.0.0.1:8250/buy &   # then, 1 s in: kill -9 billing
    # start billing again with BILLING_PAUSE=0, wait for the shop's circuit (5 s)
    curl 127.0.0.1:8250/counts                                    # invoices 0, postings 1
    curl -X POST -H 'Idempotency-Key: k2' 127.0.0.1:8250/buy      # Invoice #1
    curl 127.0.0.1:8250/counts                                    # invoices 1, postings 2 (both inv-1)
