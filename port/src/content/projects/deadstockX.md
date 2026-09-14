---

title: Dead Stock Intelligence Platform

slug: deadstockx

tagline: Detecting dead inventory and turning stock risk into financial decisions.

description: A SaaS platform that ingests inventory, sales, returns, and cost data to identify dead-stock risk and recommend financially optimized actions.

status: in progress

year: "2025"

tags:

- inventory

- SaaS

- machine-learning

- analytics

stack:

- Python

- FastAPI

- PostgreSQL

- Supabase

- Next.js

- TypeScript

- Tailwind

- XGBoost

- LightGBM

- Prophet

- Docker

- Kubernetes

github: https://github.com/Yashbhu/deadstockX

featured: false

---

  

## Overview

  

Dead Stock Intelligence is a SaaS platform for identifying slow-moving and dead inventory before it becomes a larger working-capital problem.

  

It ingests inventory, sales, returns, cost, and invoice data, scores SKUs for dead-stock risk, and turns those signals into actions such as discounts, bundles, supplier returns, and liquidation.

  

## What it does

  

- Detects at-risk and dead SKUs using business rules and planned ML scoring.

- Ingests invoice data and extracts structured inventory information.

- Supports natural-language to SQL queries for read-only analytics.

- Tracks dead-stock value, carrying cost, opportunity cost, inventory turns, and working capital.

- Generates recommendations with P&L impact analysis.

- Provides what-if simulations for inventory decisions.

- Sends alerts through email, Slack, SMS, and push notifications.

  

## Architecture

  

```text

Invoice / Inventory / Sales Data

↓

Data Extraction

↓

PostgreSQL / Supabase

↓

FastAPI Backend

↙ ↘

Risk / Forecasting Rules

↘ ↙

Recommendations

↓ ↓

Dashboard Alerts

```

  

## Current state

  

The MVP covers deterministic rules, alerts, a financial dashboard, and read-only natural-language-to-SQL querying. ML risk scoring, recommendation automation, simulations, and broader integrations are part of the ongoing roadmap.