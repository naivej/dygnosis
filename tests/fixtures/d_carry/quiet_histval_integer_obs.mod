// inventory: d_carry_quiet_histval_integer_obs
var y; varexo e; parameters a; a=.5;
model; y=a*y(-1)+e; end;
histval_file(first_obs=4,last_obs=5);
