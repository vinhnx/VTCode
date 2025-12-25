# Example: Using doc-generator in VT Code TUI

Complete step-by-step example of creating a Word document using the `doc-generator` skill in VTCode's interactive chat.

---

## Scenario: Project Proposal Document

Create a professional Word document for a software project proposal with executive summary, scope, timeline, and budget.

---

## Quick Start

```bash
$ vtcode chat
/skills load doc-generator
```

Then type:

```
Create a professional project proposal document for a mobile app development project:

Project Overview:
- Project: Customer Analytics Mobile App
- Client: TechCorp Inc.
- Duration: 6 months
- Budget: $250,000

Executive Summary (1-2 paragraphs)
- High-level project goals
- Expected ROI

Project Scope:
1. Mobile app development (iOS & Android)
2. Cloud backend infrastructure
3. Analytics dashboard
4. User authentication system

Timeline:
- Month 1-2: Design & Architecture
- Month 2-3: Core Development
- Month 4: Testing & QA
- Month 5: Deployment & Integration
- Month 6: Training & Documentation

Budget Breakdown:
- Development: $150,000
- Infrastructure: $50,000
- Testing & QA: $30,000
- Project Management: $20,000

Success Metrics:
- User adoption rate > 80%
- System uptime > 99.5%
- App rating > 4.5 stars

Risk Assessment:
- Technical risks: Low (experienced team)
- Schedule risks: Low (buffer included)
- Resource risks: Managed

Next Steps:
- Client review & approval (1 week)
- Kickoff meeting (2 weeks)
- Development starts (3 weeks)
```

---

## Full Step-by-Step Walkthrough

### Step 1: Start Chat

```bash
$ vtcode chat
```

### Step 2: Load Skill

```
/skills load doc-generator
```

**Result:** Skill loaded

### Step 3: Create Document

Type detailed requirements (as shown above)

### Step 4: Agent Creates Document

```
Agent: Creating professional project proposal document...

 Document created successfully!

The Word document contains:

Title Page:
- Project Title: "Customer Analytics Mobile App"
- Client: "TechCorp Inc."
- Date: December 13, 2024
- Professional formatting with company branding space

Executive Summary (Page 1):
- Overview of project goals and expected outcomes
- Key benefits and ROI projection
- Formatted with heading styles

Project Scope (Page 2):
1. Mobile App Development
   - iOS and Android platforms
   - Native performance optimization
   - Offline capabilities

2. Cloud Backend Infrastructure
   - Microservices architecture
   - RESTful API design
   - Database design and optimization

3. Analytics Dashboard
   - Real-time data visualization
   - Custom report generation
   - Performance metrics tracking

4. User Authentication System
   - OAuth 2.0 integration
   - Multi-factor authentication
   - Role-based access control

Timeline (Page 3):
- Month 1-2: Design & Architecture
- Month 2-3: Core Development
- Month 4: Testing & QA
- Month 5: Deployment & Integration
- Month 6: Training & Documentation

[Formatted with clear section headers and bullet points]

Budget Breakdown (Page 4):
- Development: $150,000 (60%)
- Infrastructure: $50,000 (20%)
- Testing & QA: $30,000 (12%)
- Project Management: $20,000 (8%)
- Total: $250,000

[Table format with clear organization]

Success Metrics (Page 5):
- User Adoption Rate: > 80%
- System Uptime: > 99.5%
- App Rating: > 4.5 stars

Risk Assessment (Page 5):
- Technical Risks: Low (experienced team)
- Schedule Risks: Low (buffer included)
- Resource Risks: Managed (contingency planning)

Next Steps (Page 6):
1. Client review and approval (1 week)
2. Kickoff meeting (2 weeks)
3. Development starts (3 weeks)

File Details:
- Format: .docx (Word document)
- Pages: 6
- Formatting: Professional styles, colors, spacing
- Tables: 2 (Budget breakdown, Success metrics)
- Text: Fully formatted with headings, bold, italics

Download File ID: file-XyZ789ABC123

Ready to download and send to client!
```

---

## What You Got

A professional Word document with:

**Title Page** - Project name, client, date
**Executive Summary** - Overview and key benefits
**Project Scope** - Detailed requirements (4 areas)
**Timeline** - 6-month development schedule
**Budget** - Breakdown by category (table format)
**Success Metrics** - Clear goals and KPIs
**Risk Assessment** - Risk identification and mitigation
**Next Steps** - Action items with timelines

All formatted professionally with:

-   Proper heading styles
-   Bold and italic emphasis
-   Tables with clear formatting
-   Appropriate spacing
-   Professional fonts

---

## Modifications You Can Request

```
User: Add the following to the proposal:

1. Add a "Assumptions & Constraints" section
2. Add team member bios with photos
3. Change color scheme to match client branding (blue/gold)
4. Add company logo to title page
5. Add page numbers and headers

Agent: [Updates document with requested changes]
```

---

## Other Document Types

### Meeting Minutes

```
User: Create meeting minutes document for our Q1 planning meeting:

Date: December 13, 2024
Attendees: John (PM), Sarah (Dev), Mike (Design), Lisa (QA)
Duration: 2 hours

Agenda:
1. Q1 2025 Planning
2. Resource Allocation
3. Risk Review
4. Team Building Event

Discussion Notes:
[Provide discussion points]

Action Items:
- Sarah: Complete architecture doc (Due: Dec 20)
- Mike: Create design mockups (Due: Dec 18)
- John: Resource planning (Due: Dec 22)

Decision Log:
- Approved: New development methodology
- Approved: Budget for training

Next Meeting: January 10, 2025

Agent: [Creates formatted meeting minutes document]
```

### API Documentation

```
User: Create API documentation for our REST service:

API: User Management Service
Base URL: https://api.example.com/v1

Endpoints:
1. GET /users - List all users
2. GET /users/{id} - Get user by ID
3. POST /users - Create new user
4. PUT /users/{id} - Update user
5. DELETE /users/{id} - Delete user

Authentication:
- Bearer token authentication
- JWT format

Request/Response Examples:
[Provide examples for each endpoint]

Error Codes:
- 400: Bad Request
- 401: Unauthorized
- 404: Not Found
- 500: Server Error

Rate Limiting:
- 1000 requests/hour per token

Agent: [Creates professional API documentation]
```

### Training Manual

```
User: Create a training manual for our new CRM system:

Table of Contents:
- Getting Started
- User Interface Overview
- Key Features
- Common Tasks
- Troubleshooting
- FAQ

Chapters with:
- Step-by-step instructions
- Screenshots/diagrams references
- Examples and use cases
- Tips and best practices

Target Audience: Sales team (no technical background)

Agent: [Creates comprehensive training manual]
```

### Employee Handbook

```
User: Create company handbook sections for:

1. Company Mission & Values
2. Code of Conduct
3. Remote Work Policy
4. Time Off Policy
5. Benefits Overview
6. Conflict Resolution
7. Termination Policy

Format:
- Professional layout
- Clear sections
- Easy to read
- Legally reviewed format

Agent: [Creates employee handbook]
```

---

## Real-World Use Cases

### Use Case 1: Client Proposal

-   Professional presentation to potential client
-   Details project scope and timeline
-   Shows budget and deliverables
-   Includes success metrics

### Use Case 2: Internal Report

-   Executive summary for leadership
-   Key findings and recommendations
-   Data-driven analysis
-   Action items

### Use Case 3: Training Material

-   Step-by-step guides
-   Procedures and best practices
-   Troubleshooting guides
-   FAQs

### Use Case 4: Documentation

-   API specifications
-   System architecture
-   User guides
-   Technical specifications

### Use Case 5: Meeting Minutes

-   Discussion notes
-   Action items with owners
-   Decisions made
-   Next steps

---

## Tips & Tricks

### Tip 1: Iterate and Improve

```
First version:
User: Create basic proposal

Then refine:
User: Make it more professional with:
      - Better formatting
      - Additional sections
      - Company branding

Agent: [Updated version]
```

### Tip 2: Reuse and Customize

```
User: Create a new proposal for a different client
      using the same structure as the first one,
      but with these changes:
      - Client name: NewTech Solutions
      - Budget: $500,000
      - Timeline: 12 months

Agent: [Creates adapted version]
```

### Tip 3: Combine with Other Skills

```
/skills load doc-generator
/skills load spreadsheet-generator
/skills load pdf-report-generator

User: Create a complete project report:
      1. Word document with analysis
      2. Excel spreadsheet with data
      3. PDF summary for distribution

Agent: [Creates all three files]
```

### Tip 4: Request Specific Formatting

```
User: Format the document with:
      - Company logo on title page
      - Blue color scheme
      - Professional fonts (Calibri/Arial)
      - Page numbers
      - Table of contents
      - Footer with "Confidential"

Agent: [Creates with all formatting]
```

---

## Common Requests

### Request 1: Add Table of Contents

```
User: Add an automated table of contents
      at the beginning of the document

Agent: [Adds TOC with page numbers]
```

### Request 2: Add Appendix

```
User: Add appendix sections for:
      1. Detailed specifications
      2. Research references
      3. Pricing comparison

Agent: [Adds appendix with all sections]
```

### Request 3: Add Visual Elements

```
User: Add visual elements:
      - Organization chart
      - Process flowchart
      - Timeline diagram

Agent: [Creates ASCII art or reference placeholders]
```

### Request 4: Professional Branding

```
User: Brand the document with:
      - Company logo (provide image)
      - Color scheme: #003366 (dark blue)
      - Font: Helvetica Neue
      - Tagline: "Innovation Through Technology"

Agent: [Applies branding throughout]
```

---

## Keyboard Shortcuts

| Key           | Action           |
| ------------- | ---------------- |
| `Shift+Enter` | Multi-line input |
| `Ctrl+A`      | Start of line    |
| `Ctrl+E`      | End of line      |
| `Tab`         | Autocomplete     |

---

## Download and Use

After the document is created:

1. **Download:** Use file ID provided
2. **Edit:** Open in Microsoft Word
3. **Share:** Email or present to stakeholders
4. **Print:** Professional printing ready
5. **Convert:** Save as PDF if needed

---

## Troubleshooting

### Issue: "Document structure not quite right"

**Solution:**

```
User: Reorganize the document:
      - Move sections in this order
      - Add subsections
      - Adjust formatting

Agent: [Recreates with new structure]
```

### Issue: "Too long/too short"

**Solution:**

```
User: Make it more concise (remove details)
      OR
      Make it more comprehensive (add details)

Agent: [Adjusts length]
```

### Issue: "Need different sections"

**Solution:**

```
User: Replace the Budget section with:
      [New section details]

Agent: [Updates document]
```

---

## Summary

### Quick Process

1. Start chat: `vtcode chat`
2. Load skill: `/skills load doc-generator`
3. Type requirements
4. Agent creates document
5. Download and use

### Time Estimate

-   Simple document: 30-60 seconds
-   Complex document: 1-2 minutes
-   With modifications: 2-3 minutes

### Best For

-   Professional proposals
-   Reports and documentation
-   Training materials
-   Meeting minutes
-   Project documentation
-   Employee handbooks

---

## Related Examples

-   `EXAMPLE_SPREADSHEET_GENERATOR.md` - Excel creation
-   `EXAMPLE_PDF_GENERATOR.md` - PDF reports
-   `AGENT_SKILLS_CLI_TUI.md` - Complete TUI guide

---

**Last Updated:** December 13, 2024
