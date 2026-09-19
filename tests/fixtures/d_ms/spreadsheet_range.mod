// inventory: d_ms_spreadsheet_range
// `xls_range=A1:B10` is one option value, not two option rows, and 7.1 accepts it.
// The value keeps the whole `A1:B10` span, so nothing reads `B10` as an option.
var y c k;
varexo e;
parameters alpha beta;
alpha = 0.36;
beta = 0.99;
model;
c = alpha*y + beta*c(-1) + e;
y = beta*y(-1) + c;
k = y;
end;
initval;
y = 0;
c = 0;
k = 0;
end;
shocks;
var e; stderr 0.1;
end;
data(file='x.xlsx', xls_sheet=Sheet1, xls_range=A1:B10);
