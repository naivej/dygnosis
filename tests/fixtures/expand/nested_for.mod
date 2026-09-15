// inventory: expand_nested_for
var x;
@#define is = 1:2
@#define js = 1:2
model;
@#for i in is
@#for j in js
x = @{i};
@#endfor
@#endfor
end;
