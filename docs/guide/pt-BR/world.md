---
id: world
title: Mundo: personagens, lugares e objetos
group: production
place: world
tour: world
---

# Mundo: personagens, lugares e objetos

O **Mundo** guarda o que volta nos seus vídeos: personagens (uma avó que conta as histórias da família, um detetive, um mascote), lugares (a cozinha dela, o escritório do detetive) e objetos (uma moto, um violão, um relógio de bolso). Cada um tem uma aparência fixa, notas que sempre ou nunca valem e uma **imagem de referência** por visual, para que todo vídeo que o escolhe mostre o mesmo rosto, cômodo ou coisa. O mundo do projeto fica em [Mundo](bardo:go/world); o que qualquer projeto pode escolher fica na [aba Mundo da biblioteca global](bardo:go/library/world). Escrever e editar não custa nada; desenhar uma imagem é pago e mostra o custo antes. [Mostre a tela para mim](bardo:tour/world).

<a id="kinds"></a>
## Personagens, lugares e objetos

A tela tem uma aba para cada: **Personagens**, **Lugares** e **Objetos**. Eles compartilham o mesmo formulário (nome, aparência, notas) e as mesmas imagens, com estas diferenças:

- Um **personagem** é sempre uma pessoa original, nunca uma real. As imagens dele são retratos, e ele lista seus [pertences](#belongings).
- Um **lugar** é onde as cenas acontecem. A aparência dele se chama **Descrição** (disposição, luz, materiais) e as imagens são largas, no formato de um quadro paisagem. Uma cena que acontece em um lugar escolhido é desenhada com a imagem dele.
- Um **objeto** é algo que as cenas mostram. Ele pode [pertencer a um personagem](#belongings). Uma cena que mostra um objeto escolhido é desenhada com a imagem dele quando há uma no visual do vídeo, ou a partir da descrição, se não houver.

O nome é único por tipo: um lugar e um objeto podem se chamar *Da Rosa*.

<a id="characters"></a>
## Seus personagens

A lista guarda os itens da aba aberta, do projeto aberto (ou da biblioteca global, na aba dela). **Novo personagem**, **Novo lugar** ou **Novo objeto** abre um formulário vazio; escolher um item o abre para edição. Os arquivados ficam escondidos até você marcar **Mostrar arquivados**. O nome de um item é único entre os itens do mesmo tipo e do mesmo dono, arquivados incluídos, sem diferenciar maiúsculas: um projeto e a biblioteca global podem ter, cada um, uma Rosa.

<a id="appearance"></a>
## Aparência e notas

- **Nome** (obrigatório): no máximo 60 caracteres. O plano de cenas chama o personagem por esse nome.
- **Aparência** (obrigatória): rosto, cabelo, porte e roupas, no máximo 1.500 caracteres. Toda imagem de referência é desenhada a partir deste texto, e o plano de cenas o copia como está para o elenco.
- **Sempre** e **Nunca**: o que sempre vale (carrega um guarda-chuva vermelho) e o que nunca acontece (nunca fala em cena), no máximo 1.000 caracteres cada. O planejador de cenas as lê em todo vídeo que escolhe o personagem.

Os mesmos campos valem para lugares e objetos. Uma nova **aparência** descarta as imagens de referência do item em todos os visuais e os retratos dos vídeos que o mostram, porque deixam de corresponder. Antes de salvar, o Bardo diz quais visuais e quantos vídeos: **Salvar e descartar** confirma. Cada imagem é desenhada de novo, e paga de novo, na próxima vez em que for preciso. Um novo nome ou novas notas mantêm as imagens.

<a id="belongings"></a>
## Pertences

Um objeto pode pertencer a um personagem: no objeto, **Pertence a** indica o personagem, ou **Ninguém**. Isso é salvo assim que você escolhe. O objeto de um projeto pode pertencer a um personagem do projeto ou da biblioteca global; um objeto global, só a um personagem global. O formulário do personagem lista os **Pertences** dele, e o planejador de cenas os lê junto com o personagem.

Em [Quem aparece](scenes.md#who-appears), escolher um personagem oferece primeiro os pertences dele, em **Pertences dos personagens escolhidos**. Um pertence nunca é escolhido sozinho: você adiciona os que este vídeo mostra.

<a id="pictures"></a>
## Imagens de referência

Uma imagem de referência é o retrato do personagem, ou a imagem do lugar ou do objeto, em um **visual**: um estilo do catálogo (foto, desenho animado, mangá e assim por diante), ou **as notas de um canal**, identificadas pelo canal e pelo texto das notas de estética dele. Cada imagem tem seu próprio registro de como foi feita (provedor, modelo, prompt, quando, e se um vídeo a desenhou, você a desenhou aqui, a salvou do elenco de um vídeo ou a importou), e fica na pasta `World` do projeto, ou em `Global\World` dentro da pasta base para a biblioteca global.

- **Antes da primeira cena que precisa dela**: um vídeo que mostra o personagem em um visual sem imagem desenha uma antes dessa cena, como faz com qualquer [pessoa que volta](scenes.md#cast), paga nesse vídeo. A imagem então fica guardada no mundo.
- **Sob demanda**: escolha um visual, veja a estimativa e clique em **Desenhar imagem**. A nova imagem aguarda revisão: **Manter** faz dela a que os vídeos nesse visual usam, no lugar da anterior, e **Descartar** a joga fora. Ela é paga no projeto do personagem; a de um personagem global, em nenhum projeto.

<a id="import"></a>
## Importar uma imagem

A imagem de um lugar ou de um objeto também pode ser **importada**: escolha o visual, clique em **Importar imagem…**, confirme que pode usar a imagem nos seus vídeos e que ela não mostra nenhuma pessoa real, e então em **Escolher imagem…** (PNG, JPEG ou WebP, dentro dos limites de uma [imagem de produto](products.md#pictures)). Ela vira na hora a imagem mantida desse visual, no lugar da que estiver lá, e guarda sua confirmação com a data. Importar não custa nada. O retrato de um personagem não pode ser importado.

<a id="reuse"></a>
## Reaproveitar uma imagem

Todo vídeo seguinte no mesmo visual recebe a imagem mantida de graça: a estimativa não a conta e nenhuma chamada paga é feita. Outras notas em um canal são outro visual, então desenham uma nova imagem. Excluir o vídeo que desenhou a imagem primeiro mantém a imagem. **Remover** descarta uma imagem mantida; o próximo vídeo nesse visual desenha uma nova.

<a id="archive"></a>
## Duplicar, arquivar e excluir

- **Duplicar** copia os campos e as imagens mantidas com um novo nome.
- **Arquivar** esconde o item de [Quem aparece](scenes.md#who-appears); os vídeos que já o escolheram continuam com ele. **Restaurar** o traz de volta.
- **Excluir** remove o item com suas imagens. É recusado enquanto um vídeo em andamento o usa, e a mensagem cita esses vídeos: arquive o item, ou arquive ou exclua esses vídeos antes. Os vídeos que o mostraram mantêm suas imagens; a pessoa do elenco ou o lugar passa a ser do próprio vídeo, e os objetos de um personagem excluído deixam de pertencer a ele.

<a id="sharing"></a>
## Compartilhar com outros projetos

Abaixo das ações, **Compartilhamento** marca um item como **Compartilhável** para seus outros projetos, exporta uma cópia para um deles, torna o item global ou o salva em arquivo; **Importar de outro projeto** e **Abrir arquivo de item…**, no topo, trazem um para cá. Uma cópia vinculada segue toda mudança do original, e as imagens chegam logo depois. Veja [Compartilhar entre projetos](global-library.md#sharing).

<a id="save"></a>
## Salvar uma pessoa do vídeo no mundo

Uma pessoa do elenco de um vídeo pode virar personagem: na etapa [Cenas](scenes.md#cast), **Salvar em Mundo** a guarda no mundo do projeto e **Salvar na biblioteca global** na biblioteca global, com o retrato dela no visual do vídeo. A pessoa do vídeo passa a ser esse personagem.

<a id="save-place"></a>
## Salvar um lugar do vídeo no mundo

Um lugar do plano de um vídeo pode virar um lugar do mundo do mesmo jeito: sob o lugar de uma cena, **Salvar em Mundo** ou **Salvar na biblioteca global**. O lugar do plano passa a ser esse lugar (ele mostra **Do mundo**) e fica escolhido para o vídeo. Um lugar do plano não tem imagem própria: a primeira imagem dele é desenhada quando um vídeo precisar.

<a id="backup"></a>
## Backups e o arquivo de configurações

Os [backups](backup.md) levam os personagens, lugares, objetos e suas imagens, importadas incluídas. O [arquivo de configurações](settings.md) não: ele leva preferências, não o seu trabalho. Excluir um projeto exclui o mundo dele.
