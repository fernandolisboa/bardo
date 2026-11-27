---
id: global-library
title: Biblioteca global
group: getting-started
place: library
tour: library
---

# Biblioteca global

A **Biblioteca global** guarda o que todo projeto pode usar: personagens, lugares, objetos, narradores, produtos e modelos. [Mostre-me](bardo:tour/library).

<a id="tabs"></a>
## As abas

A biblioteca tem quatro abas:

- **Mundo**: personagens, lugares e objetos que os vídeos de qualquer projeto podem escolher ([Mundo](world.md)).
- **Narradores**: as vozes e rostos que falam nos seus vídeos ([Narradores](personas.md)).
- **Produtos**: os produtos que seus anúncios mostram, com briefings e imagens ([Produtos](products.md)).
- **Modelos**: as instruções a partir das quais o Claude escreve ([Modelos](templates.md)).

<a id="libraries"></a>
## O que cada uma guarda

Cada aba é a mesma tela que você conhece, com as mesmas listas e formulários: crie, edite e arquive itens ali como antes.

<a id="projects"></a>
## Globais e do projeto

Personagens, lugares e objetos podem ser do projeto, no [Mundo](world.md) dele, ou globais, nesta aba: um vídeo escolhe entre os dois. Narradores e produtos funcionam do mesmo jeito: as entradas **Narradores** e **Produtos** de um projeto listam os dele e os globais que ele usa, marcados como **Global**, e estas abas listam os globais. Editar um item global muda ele em todo lugar onde é usado, então o Bardo lista esses lugares antes; **Trazer para o projeto** faz uma cópia do projeto em vez disso ([Seus narradores](personas.md#library), [Seus produtos](products.md#library)). Cada tipo de modelo tem um modelo global aqui, e um projeto pode ter o seu ([Do projeto ou global](templates.md#project)). Excluir um projeto exclui os narradores, produtos e modelos dele; os globais ficam.

<a id="sharing"></a>
## Compartilhar entre projetos

Os personagens, lugares, objetos, narradores, produtos e modelos de um projeto são dele, mas seus outros projetos também podem usá-los. Abaixo do formulário de um item, **Compartilhamento** mostra como:

- **Compartilhável** deixa seus outros projetos importarem o item. Desmarque e toda cópia vinculada dele vira independente.
- **Exportar para outro projeto** põe uma cópia independente, com as imagens, no projeto que você escolher.
- **Tornar global** move o item para cá, para a biblioteca global. O projeto continua usando o item, e toda cópia vinculada dele em outros projetos passa a ser o próprio item global: o que apontava para a cópia agora aponta para o global. Para um modelo, o texto vira a próxima versão do modelo global, depois que o Bardo lista os projetos que usam o global, e o projeto volta a usar o modelo global.
- **Salvar em arquivo…** grava um arquivo `.bardo-item`, para abrir em outro computador.

No topo de **Mundo**, **Narradores**, **Produtos** e **Modelos** de um projeto, **Importar de outro projeto** lista o que seus outros projetos marcaram como **Compartilhável**, inclusive projetos arquivados. Escolha **Cópia vinculada** ou **Cópia independente**:

- Uma **cópia vinculada** segue toda mudança do original, no mesmo salvamento: é só leitura, tem a etiqueta **Vinculado** e diz o que segue. Um modelo vinculado recebe cada nova versão do original. As imagens de um item do mundo chegam logo depois; enquanto são copiadas, ele mostra **Atualizando**, e **Aguardando a pasta** enquanto uma pasta está num disco desconectado. **Desvincular** torna a cópia do projeto, com tudo o que ela tem. Quando o original é excluído, deixa de ser compartilhado ou o projeto dele é excluído, a cópia vira independente com tudo o que tinha, e avisa uma vez.
- Uma **cópia independente** é do projeto desde o início. Um modelo copia só a versão mais recente.

Quando o projeto já tem um com o mesmo nome, o Bardo sugere o nome seguido do nome do outro projeto. **Abrir arquivo de item…** abre um `.bardo-item` (ou o `.bardo-persona` de um narrador) como cópia independente. Um arquivo nunca guarda foto de uma pessoa real: a aparência de um narrador feita com foto fica de fora. O arquivo de um produto guarda as imagens dele, então abrir um pede que você confirme que pode usar as imagens e a marca. Um arquivo feito por um Bardo mais novo é recusado: atualize o Bardo para abri-lo.
