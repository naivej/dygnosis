// inventory: d_carry_e001_icd_date_minus
var y; varexo e; parameters a; a=.5;
model; y=a*y(-1)+e; end;
initial_condition_decomposition(plot_end_date=2000Q1-1) y;
