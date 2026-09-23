// inventory: d_carry_e001_data_date_minus
var y; varexo e; parameters a; a=.5;
model; y=a*y(-1)+e; end;
data(file='x.csv',first_obs=2000Q1-1);
