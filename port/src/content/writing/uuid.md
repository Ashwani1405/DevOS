---
title: UUID
slug: forgequeue-uuid
published: true
date: 2026-06-16
description: What UUIDs are, how they are generated, and why they avoid central coordination.
tags:
  - distributed systems
readingTime: 2 min
links:
  - ForgeQueue
---
t is a 128-bit number used to uniquely identify information or objects in computer systems. generated how? either by timestamps or a large string Computer systems generate UUIDs locally using very large random numbers theoritically it cant be same but in reality its probablity is very small though if we want absolute unique we can assign a central authority

represented by 36-character string in the format `123e4567-e89b-12d3-a456-426614174000` removes centralised use of database as it can help in tracking entire lifescyle without communciating through central database generated in memory or locally saves id collision

SNOWFLAKE ID

ULID