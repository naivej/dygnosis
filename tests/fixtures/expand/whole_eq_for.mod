// inventory: expand_whole_eq_for
var y;
@#define is = 1:3
model;
@#for i in is
y = @{i};
@#endfor
end;
