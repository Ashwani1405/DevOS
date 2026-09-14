---

title: OSINT Investigator

slug: osint-investigator

tagline: Searching public information and turning investigation results into structured data.

description: A real-time OSINT investigation tool that searches public sources for a person and location, extracts entities with NLP, scores results, and exports structured JSON.

status: archived

year: "2025"

tags:

- OSINT

- NLP

- information-retrieval

- investigation

stack:

- Python

- spaCy

- Google Custom Search API

- NLP

- JSON

github: https://github.com/Yashbhu/OSINT

featured: false

---

  

## Overview

  

OSINT Investigator is a Python-based investigation tool for searching publicly available information and turning the results into structured data.

  

Given a name and city, the system performs live searches, extracts relevant entities using NLP, scores the results, and saves the investigation output as JSON.

  

## Pipeline

  

```text

Name + City

↓

Public Web Search

↓

Result Collection

↓

NLP Entity Extraction

↓

Relevance Scoring

↓

Structured JSON

```

  

## What it does

  

- Performs live public-web searches.

- Searches sources such as professional profiles and news/case records.

- Extracts entities using NLP.

- Scores collected results.

- Exports structured JSON.

- Uses environment variables for API configuration.

  

## Repository structure

  

```text

model/

osint_investigator.py

requirements.txt

JSON output

```