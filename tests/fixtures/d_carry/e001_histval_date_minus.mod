// inventory: d_carry_e001_histval_date_minus
var y; varexo e; parameters a; a=.5;
model; y=a*y(-1)+e; end;
histval_file(first_obs=2000Q1-1);
