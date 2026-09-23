// inventory: d_carry_e001_plot_date_minus
var y; varexo e; parameters a; a=.5;
model; y=a*y(-1)+e; end;
plot_shock_decomposition(plot_init_date=2000Q1-1) y;
