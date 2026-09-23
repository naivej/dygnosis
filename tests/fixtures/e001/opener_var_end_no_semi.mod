// inventory: e001_opener_var_end_no_semi
// An `end` written without its `;`: 7.1's lexer returns to `INITIAL` on the `end`
// word itself, so the next block opener is the opener token again and the file is
// refused `syntax error, unexpected INITVAL, expecting ';'`. The body-flush rule
// must still terminate the model body, so this keeps **E001**.
var y;
varexo e;
parameters rho;
rho = 0.5;

model;
y = rho * y(-1) + e;
end
initval;
y = 0;
end;
