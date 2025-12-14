# Example: Using spreadsheet-generator in VTCode TUI

Complete step-by-step example of creating an Excel spreadsheet using the `spreadsheet-generator` skill in VTCode's interactive chat.

---

## Scenario: Q4 2024 Financial Dashboard

Create a professional Excel spreadsheet with quarterly financial data, including revenue, expenses, and profit analysis.

---

## Step 1: Start the Chat Interface

```bash
$ vtcode chat
```

**You'll see:**
```

          VTCode Interactive Chat Session            

                                                      
  Welcome! Ready to chat.                            
  Type /help for commands, or start typing           
                                                      

 Input: _                                            

```

---

## Step 2: List Available Skills

```
Type: /skills list
Press: Enter
```

**Output:**
```

                                                      
 User: /skills list                                  
                                                      
 Agent: Available Skills:                            
                                                      
 • spreadsheet-generator (v1.0.0)                    
   Generate professional Excel spreadsheets with     
   data, charts, and formatting                      
                                                      
 • doc-generator (v1.0.0)                            
   Generate professional Word documents with         
   formatted text, tables, and layouts               
                                                      
 • pdf-report-generator (v1.0.0)                     
   Generate professional PDF documents with          
   charts, styling, and complex layouts              
                                                      
 • strict-architecture                               
   Enforces universal strict governance rules        
                                                      
 Use '/skills info <name>' for details               
                                                      

 Input: _                                            

```

---

## Step 3: Get Skill Details (Optional)

```
Type: /skills info spreadsheet-generator
Press: Enter
```

**Output:**
```

                                                      
 User: /skills info spreadsheet-generator            
                                                      
 Agent: Skill: spreadsheet-generator                 
 Description: Generate professional Excel            
 Version: 1.0.0                                      
 Author: VTCode Team                                 
                                                      
 --- Instructions ---                                
 When asked to generate a spreadsheet:               
                                                      
 1. Understand Requirements                          
    Parse request for content, structure, format     
                                                      
 2. Plan the Spreadsheet                             
    Sketch layout and column structure               
    Identify calculations and dependencies           
    Determine chart types and data ranges            
    Plan summary/dashboard sheets if needed          
                                                      
 3. Create with Code Execution                       
    Use Anthropic xlsx Agent Skill to generate       
    file with proper structure and formatting        
                                                      
 4. Extract File Reference                           
    Locate file_id in response                       
    Return file reference for download               
                                                      
 --- Examples                                        
 • Financial Dashboard - Create Q4 analysis          
 • Sales Analysis - Regional performance data        
 • Inventory Tracking - SKU management system        
                                                      
 --- Features Supported                              
 • Multiple Sheets - Organize data across sheets     
 • Formatting - Colors, fonts, number formats        
 • Formulas - SUM, AVERAGE, IF, VLOOKUP             
 • Charts - Bar, line, pie charts with labels        
 • Tables - Formatted data ranges with filters       
 • Conditional Formatting - Highlight cells         
 • Merged Cells - Professional layout                
                                                      

 Input: _                                            

```

**Understanding the skill:**
-  Generates Excel spreadsheets
-  Supports multiple sheets
-  Includes formulas and calculations
-  Can create charts
-  Professional formatting

---

## Step 4: Load the Skill

```
Type: /skills load spreadsheet-generator
Press: Enter
```

**Output:**
```

                                                      
 User: /skills load spreadsheet-generator            
                                                      
 Agent:  Loaded skill: spreadsheet-generator        
        (v1.0.0)                                      
        Description: Generate professional Excel     
        spreadsheets with data, charts, and          
        formatting                                   
        Resources: 0 files                           
                                                      
        Skill is ready to use. Use it in your next   
        message.                                     
                                                      

 Input: _                                            

```

**What happened:**
- Skill metadata loaded (~100 tokens)
- Agent now recognizes the skill
- Ready to use in your next request

---

## Step 5: Use the Skill

Now type your request to create the spreadsheet:

```
Type: Create an Excel spreadsheet with Q4 2024 financial data:

Sheet 1 - Monthly Revenue:
- Column A: Month names (Oct, Nov, Dec)
- Column B: Revenue in dollars (October: $450,000, November: $520,000, December: $620,000)
- Column C: Month-over-month growth percentage
- Format header row with bold and light blue background

Sheet 2 - Expense Breakdown:
- Categories: Salaries (40%), Operations (25%), Marketing (20%), Other (15%)
- Show percentages of total revenue ($1,590,000)
- Include pie chart showing expense distribution
- Calculate absolute dollar amounts

Sheet 3 - Summary Dashboard:
- Key metrics: Total Revenue, Total Expenses, Net Profit, Profit Margin
- Create nice formatting with larger fonts for metrics
- Add color coding: green for profit, red for expenses

Press: Enter
```

**Agent Response:**
```

                                                      
 User: Create an Excel spreadsheet with Q4 2024     
       financial data...                             
                                                      
 Agent: I'll create a comprehensive Q4 2024          
 financial dashboard spreadsheet using the           
 spreadsheet-generator skill...                      
                                                      
 [Thinking...]                                      
                                                      
 Planning the spreadsheet structure:                 
 • Sheet 1: Monthly revenue with growth trends       
 • Sheet 2: Expense breakdown with pie chart         
 • Sheet 3: Summary dashboard with KPIs              
                                                      
 Creating the spreadsheet...                         
                                                      
  Spreadsheet created successfully!                 
                                                      
 The Excel file (Q4_2024_Financial_Dashboard.xlsx)   
 contains:                                           
                                                      
 Sheet 1 - Monthly Revenue                           
 • Oct: $450,000 (baseline)                          
 • Nov: $520,000 (+15.6% growth)                     
 • Dec: $620,000 (+19.2% growth)                     
 • Q4 Total: $1,590,000                              
 • Header: Bold text, light blue background          
 • Professional formatting with $ currency format    
                                                      
 Sheet 2 - Expense Breakdown                         
 • Salaries: $636,000 (40%)                          
 • Operations: $397,500 (25%)                        
 • Marketing: $318,000 (20%)                         
 • Other: $238,500 (15%)                             
 • Total Expenses: $1,590,000                        
 • Pie Chart: Visual expense distribution            
 • Color-coded by category                           
                                                      
 Sheet 3 - Summary Dashboard                         
 • Total Revenue: $1,590,000 (green, 18pt bold)      
 • Total Expenses: $1,590,000 (red, 18pt bold)       
 • Net Profit: $0 (yellow, 16pt bold)                
 • Profit Margin: 0% (yellow, 16pt bold)             
 • Clear formatting with color coding                
 • Easy-to-read metrics display                      
                                                      
 File Details:                                       
 • Format: .xlsx (Excel format)                      
 • Sheets: 3                                         
 • Charts: 1 (pie chart)                             
 • Formulas: Growth % calculations                   
 • Formatting: Bold headers, colors, fonts           
                                                      
 Download File ID: file-ABcd123XyZ456                
                                                      
 The spreadsheet is ready! You can:                  
 • Download it for use in Excel                      
 • Share with your team                              
 • Import into other tools                           
 • Edit further in Excel if needed                   
                                                      

 Input: _                                            

```

---

## What You Got

The agent created an Excel file with:

### Sheet 1: Monthly Revenue
```
Month       Revenue      Growth %
Oct      $450,000         -
Nov      $520,000      +15.6%
Dec      $620,000      +19.2%
```

### Sheet 2: Expense Breakdown
```
Category        Amount      Percentage
Salaries     $636,000         40%
Operations   $397,500         25%
Marketing    $318,000         20%
Other        $238,500         15%
Total      $1,590,000        100%

[Pie Chart showing distribution]
```

### Sheet 3: Summary Dashboard
```
Total Revenue        $1,590,000
Total Expenses       $1,590,000
Net Profit                  $0
Profit Margin              0%
```

---

## Step 6: Continue or Modify

You can now:

### Option A: Ask Agent to Modify

```
Type: Add a profit target sheet showing:
      - Break-even analysis
      - Target profit margin of 15%
      - Revenue needed to reach 15% profit
      
Agent: [Adds new sheet with calculations]
```

### Option B: Ask for Different Analysis

```
Type: Create another analysis comparing Q3 vs Q4

Agent: [Creates comparison sheet]
```

### Option C: Ask for Additional Charts

```
Type: Add a trend line chart showing revenue growth

Agent: [Adds line chart to Sheet 1]
```

### Option D: Download and Use

The file is ready to download at file ID: `file-ABcd123XyZ456`

---

## Tips & Tricks

### Tip 1: Create Similar Spreadsheets Quickly

Once loaded, create multiple spreadsheets:

```
User: Now create a sales report spreadsheet with:
      - Sales by region
      - Product categories
      - Year-over-year comparison

Agent: [Creates new spreadsheet without reloading skill]
```

### Tip 2: Complex Calculations

Ask for formulas:

```
User: Add a sheet with:
      - Quarterly profitability analysis
      - Running totals
      - Variance from budget

Agent: [Creates with SUM, AVG, IF formulas]
```

### Tip 3: Professional Formatting

Request specific styling:

```
User: Format the headers with:
      - Company logo (if available)
      - Dark blue background
      - White text
      - 14pt font

Agent: [Applies professional formatting]
```

### Tip 4: Multiple Charts

Ask for visualizations:

```
User: Add three charts:
      1. Revenue trend line
      2. Expense breakdown pie
      3. Monthly comparison bar chart

Agent: [Creates all three charts]
```

---

## Real-World Examples

### Example 1: Sales Report

```
User: Create a sales spreadsheet with:
      - Weekly sales data for last 12 weeks
      - Sales by product line
      - Top performing regions
      - Week-over-week growth %
      - Sales forecast for next 4 weeks
      
Agent: [Creates comprehensive sales report]
```

### Example 2: Budget Analysis

```
User: Create a budget vs actual spreadsheet:
      - Department budgets
      - Actual spending
      - Variance (positive/negative)
      - Percentage of budget used
      - Alerts for overspent items

Agent: [Creates budget analysis with conditional formatting]
```

### Example 3: Inventory Tracking

```
User: Create inventory management spreadsheet:
      - SKU and product names
      - Current quantity
      - Reorder level
      - Cost per unit
      - Total value
      - Status (In Stock, Low Stock, Reorder)

Agent: [Creates inventory tracker with formulas]
```

### Example 4: Project Timeline

```
User: Create a project tracking spreadsheet:
      - Task names
      - Start and end dates
      - % complete
      - Assigned to
      - Status (On Track, At Risk, Complete)
      - Gantt chart visualization

Agent: [Creates project tracker with timeline chart]
```

---

## Keyboard Shortcuts During Input

| Key | Action |
|-----|--------|
| `Shift+Enter` | Continue typing (multi-line input) |
| `Ctrl+A` | Start of line |
| `Ctrl+E` | End of line |
| `Ctrl+W` | Delete previous word |
| `Tab` | Autocomplete |

---

## Common Questions

### Q: Can I edit the file afterwards?
**A:** Yes! Download the Excel file and edit it in Microsoft Excel, Google Sheets, or any spreadsheet app.

### Q: Can I create multiple spreadsheets?
**A:** Yes! The skill stays loaded, so you can create more by typing new requests.

### Q: What if I need to modify it?
**A:** Ask the agent: "Add [feature] to the spreadsheet" or "Create an updated version with..."

### Q: Can I combine it with other skills?
**A:** Yes! Load multiple skills and ask for both spreadsheet and document generation.

### Q: How long does it take to create?
**A:** Usually 30-60 seconds depending on complexity.

### Q: Is there a file size limit?
**A:** Max ~50MB in code execution environment (way more than typical spreadsheets).

---

## Troubleshooting

### Issue: "File not found" or "Generation failed"

**Solution:**
```
User: Try again, but with simpler data structure

Agent: [Creates with simpler format]
```

### Issue: "Missing features"

**Solution:**
```
User: Add the missing feature to the spreadsheet:
      [Specify what's missing]

Agent: [Updates spreadsheet]
```

### Issue: "Formatting not right"

**Solution:**
```
User: Change the formatting to:
      [Specify desired formatting]

Agent: [Recreates with proper formatting]
```

---

## Summary

### Quick Reference
1. **Start:** `vtcode chat`
2. **List:** `/skills list`
3. **Load:** `/skills load spreadsheet-generator`
4. **Create:** Type your spreadsheet request
5. **Download:** Get file from file ID

### Key Points
-  Professional Excel files created instantly
-  Multiple sheets supported
-  Formulas and calculations included
-  Charts and visualizations available
-  Professional formatting applied
-  Editable after download
-  Can create multiple spreadsheets
-  Easy to modify or iterate

### Time to Complete
- **Simple spreadsheet:** 30-60 seconds
- **Complex spreadsheet:** 1-2 minutes
- **Multiple sheets:** 2-3 minutes

---

## Next Steps

1. **Try it now:** `vtcode chat`
2. **Load skill:** `/skills load spreadsheet-generator`
3. **Create your own:** Type your spreadsheet requirements
4. **Iterate:** Ask agent to refine or add features
5. **Download:** Use the generated file

---

## Related Examples

- `EXAMPLE_DOC_GENERATOR.md` - Word document generation
- `EXAMPLE_PDF_GENERATOR.md` - PDF report generation
- `AGENT_SKILLS_CLI_TUI.md` - Complete TUI guide

---

**Last Updated:** December 13, 2024
