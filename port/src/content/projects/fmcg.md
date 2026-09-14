---

title: FMCG Tender Intelligence Platform

slug: fmcg

tagline: Collecting and analyzing FMCG tenders and procurement data.

description: A full-stack procurement intelligence platform for collecting, processing, searching, and analyzing government and marketplace tenders and RFPs.

status: in progress

year: "2025"

tags:

- procurement

- tenders

- data-ingestion

- analytics

stack:

- Next.js

- React

- TypeScript

- Tailwind

- Python

- FastAPI

- SQLite

- pandas

- Playwright

- BeautifulSoup

github: https://github.com/Yashbhu/fmcg

featured: true

---

  

## Overview

  

FMCG is a procurement and tender intelligence platform focused on collecting, processing, and presenting government and marketplace tenders and RFPs.

  

The application separates the frontend, backend, data/model layer, and database so ingestion and analysis workflows can evolve independently from the user interface.

  

## What it does

  

- Ingests tenders from CSV and scraped sources.

- Stores and queries tender data.

- Provides search, filtering, and analytics.

- Exposes FastAPI endpoints for analysis and data operations.

- Contains pricing and workflow experimentation.

- Includes agent-oriented logic for pricing, sales, and technical analysis.

- Uses Playwright and BeautifulSoup for scraping and testing workflows.

  

## Architecture

  

```text

Tender Sources

↓

Ingestion / Processing

↓

SQLite / Data Layer

↓

FastAPI Backend

↓

Next.js / React UI

```

  

## Repository structure

  

```text

frontend/ Next.js + TypeScript UI

backend/ FastAPI API

model/ ingestion, pricing and workflow logic

database/ SQLite development database

```